//! rclone as the [`Engine`]: files sent and checked, listed, downloaded,
//! restored, deleted and shared.

use super::*;
use link_format::WithinExt;

/// Whether `a` and `b` are the same file on disk (links followed).
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
    }
}

impl Engine for Rclone {
    /// Clouds side by side, each in a thread with an engine of its own,
    /// after asking here, one at a time, what each would otherwise ask
    /// again: the Keychain, sign-ins, and which checksums a cloud has.
    fn send_and_check(&self, root: &Path, sending: &[Sending]) -> Vec<Sent> {
        if sending.len() < 2 {
            return sending
                .iter()
                .map(|one| send_and_check_one(self, root, one))
                .collect();
        }
        // An error here is asked again, and said, in each thread.
        let _ = self.keychain_settings();
        for one in sending {
            // An error here is asked again, and said, in the thread.
            let _ = self.sign_ins_for(&[&one.cloud.remote()]);
            let _ = self.hashes(&one.cloud.plain_remote());
        }
        std::thread::scope(|scope| {
            let threads: Vec<_> = sending
                .iter()
                .map(|one| {
                    let mut engine = self.duplicate();
                    engine.beside = Some(one.cloud.name.clone());
                    scope.spawn(move || send_and_check_one(&engine, root, one))
                })
                .collect();
            threads
                .into_iter()
                .zip(sending)
                .map(|(thread, one)| {
                    thread.join().unwrap_or_else(|_| Sent {
                        failed: None,
                        checked: Err(anyhow::anyhow!(
                            "sending to {} stopped unexpectedly",
                            one.cloud.name
                        )),
                    })
                })
                .collect()
        })
    }

    /// As [`Engine::send_and_check`]: each cloud in a thread with an
    /// engine of its own, what every cloud asks for warmed once first. A
    /// cloud whose thread stops unexpectedly is left out.
    fn each_cloud(&self, clouds: &[Cloud], each: &(dyn Fn(&dyn Engine, usize) + Sync)) {
        if clouds.len() < 2 {
            for i in 0..clouds.len() {
                each(self, i);
            }
            return;
        }
        // An error here is asked again, and said, in each thread.
        let _ = self.keychain_settings();
        for cloud in clouds {
            let _ = self.sign_ins_for(&[&cloud.remote()]);
            let _ = self.hashes(&cloud.plain_remote());
        }
        std::thread::scope(|scope| {
            let threads: Vec<_> = clouds
                .iter()
                .enumerate()
                .map(|(i, cloud)| {
                    let mut engine = self.duplicate();
                    engine.beside = Some(cloud.name.clone());
                    scope.spawn(move || each(&engine, i))
                })
                .collect();
            for thread in threads {
                let _ = thread.join();
            }
        });
    }

    fn upload(
        &self,
        cloud: &Cloud,
        root: &Path,
        files: &[String],
        options: &Upload,
    ) -> anyhow::Result<()> {
        let root = root.to_string_lossy();
        let remote = cloud.remote();
        let mut args = vec!["copy", &*root, &remote];
        if !options.replace {
            args.extend(["--checksum", "--immutable"]);
        } else if !cloud.encrypted {
            // An encrypted cloud shows rclone no checksum, so it would
            // compare sizes alone and keep a copy whose size didn't
            // change: a replace compares times and sizes there instead.
            args.push("--checksum");
        }
        let flag;
        if let Some(class) = &options.class {
            flag = class_flag(cloud, class)?;
            args.push(&flag);
        }
        args.extend(options.flags.iter().map(String::as_str));
        ensure!(
            self.run_shown(&args, files)?,
            "some files didn't reach {} (above)",
            cloud.name
        );
        Ok(())
    }

    fn pending(
        &self,
        cloud: &Cloud,
        files: &[String],
    ) -> anyhow::Result<Option<std::collections::HashSet<String>>> {
        if !uploader_hashes(cloud) || files.is_empty() {
            return Ok(Some(Default::default()));
        }
        let stored = if cloud.encrypted {
            self.encrypted_names(cloud, files)?
        } else {
            files.to_vec()
        };
        let output = self.output(&["backend", "list-multipart-uploads", &cloud.plain_remote()])?;
        let listed = match output.status.success() {
            true => String::from_utf8_lossy(&output.stdout).into_owned(),
            // rclone's own S3 server answers a bucket with none so.
            false if said(&output).contains("NoSuchUpload") => "{}".into(),
            false => return Ok(None),
        };
        Ok(pending_in(&listed, &stored).map(|keys| {
            stored
                .iter()
                .zip(files)
                .filter(|(name, _)| keys.contains(name.as_str()))
                .map(|(_, file)| file.clone())
                .collect()
        }))
    }

    fn check(
        &self,
        cloud: &Cloud,
        root: &Path,
        files: &[String],
        proof: Proof,
    ) -> anyhow::Result<Vec<(String, Found)>> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        // No checksums where the copies are stored: read them back. To
        // delete on, also those whose checksum their uploader set.
        let (read, by_sum): (Vec<String>, Vec<String>) =
            if self.hashes(&cloud.plain_remote())?.is_empty() {
                (files.to_vec(), Vec::new())
            } else if proof == Proof::ToDelete && uploader_hashes(cloud) {
                let trusted = self.hashed_by_storage(cloud, files)?;
                files
                    .iter()
                    .cloned()
                    .partition(|file| !trusted.contains(file))
            } else {
                (Vec::new(), files.to_vec())
            };
        let mut found = std::collections::HashMap::new();
        for (names, download) in [(by_sum, false), (read, true)] {
            if !names.is_empty() {
                found.extend(self.compare(cloud, root, &names, download)?);
            }
        }
        let mut results: Vec<(String, Found)> = files
            .iter()
            .map(|file| {
                let result = found
                    .remove(file)
                    .unwrap_or_else(|| Found::Error("not checked".into()));
                (file.clone(), result)
            })
            .collect();
        // A "copy" in a drive cloud that is this very file (a link in the
        // backup folder pointing back here) is no copy at all.
        if let (Kind::Drive, false, Some(path)) = (cloud.kind, cloud.encrypted, &cloud.path) {
            let stored = Path::new(path).join(&cloud.bucket);
            for (file, found) in &mut results {
                if *found == Found::Same && same_file(&root.join(&*file), &stored.join(&*file)) {
                    *found = Found::Error("the copy is this same file, through a link".into());
                }
            }
        }
        Ok(results)
    }

    fn written(
        &self,
        cloud: &Cloud,
        under: &str,
    ) -> anyhow::Result<Option<std::collections::HashMap<String, Written>>> {
        if !uploader_hashes(cloud) {
            return Ok(None);
        }
        // The time S3 wrote each object, from the listing: no request per
        // object, nothing read; only the folder the files cleaned are in.
        let remote = cloud.remote();
        let remote = match (under, remote.ends_with(':')) {
            ("", _) => remote,
            (_, true) => format!("{remote}{under}"),
            (_, false) => format!("{remote}/{under}"),
        };
        let output = self.run(&[
            "lsjson",
            "-R",
            "--files-only",
            "--no-mimetype",
            "--fast-list",
            "--use-server-modtime",
            &remote,
        ])?;
        let items: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).within("reading rclone's listing")?;
        Ok(Some(
            items
                .into_iter()
                .filter_map(|item| {
                    let path = match under {
                        "" => item["Path"].as_str()?.to_string(),
                        _ => format!("{under}/{}", item["Path"].as_str()?),
                    };
                    let written = Written {
                        size: item["Size"].as_u64()?,
                        at: item["ModTime"]
                            .as_str()
                            .filter(|t| !t.is_empty())?
                            .to_string(),
                    };
                    Some((path, written))
                })
                .collect(),
        ))
    }

    fn list(&self, cloud: &Cloud) -> anyhow::Result<Vec<Listed>> {
        let remote = cloud.remote();
        let output = self.run(&[
            "lsjson",
            "-R",
            "--files-only",
            "--no-mimetype",
            "--fast-list",
            &remote,
        ])?;
        let items: Vec<serde_json::Value> =
            serde_json::from_slice(&output.stdout).within("reading rclone's listing")?;
        Ok(items
            .into_iter()
            .filter_map(|item| {
                let path = item["Path"].as_str()?.to_string();
                if path.starts_with(".link/") {
                    return None;
                }
                Some(Listed {
                    path,
                    size: item["Size"].as_u64().unwrap_or(0),
                    class: item["Tier"]
                        .as_str()
                        .filter(|t| !t.is_empty())
                        .map(str::to_string),
                })
            })
            .collect())
    }

    fn download(
        &self,
        cloud: &Cloud,
        root: &Path,
        files: &[String],
        options: &Download,
    ) -> anyhow::Result<()> {
        let root = root.to_string_lossy();
        let remote = cloud.remote();
        let mut args = vec!["copy", &*remote, &*root, "--checksum"];
        if !options.replace {
            args.push("--immutable");
        }
        args.extend(options.flags.iter().map(String::as_str));
        ensure!(
            self.run_shown(&args, files)?,
            "some files didn't come back from {} (above)",
            cloud.name
        );
        Ok(())
    }

    fn restore(&self, cloud: &Cloud, files: &[String]) -> anyhow::Result<()> {
        let (remote, files) = if cloud.encrypted {
            (cloud.plain_remote(), self.encrypted_names(cloud, files)?)
        } else {
            (cloud.remote(), files.to_vec())
        };
        let list = FileList::new(&files)?;
        let list_path = list.path.to_string_lossy();
        self.run(&[
            "backend",
            "restore",
            &remote,
            "--files-from-raw",
            &list_path,
            "-o",
            "priority=Standard",
            "-o",
            "lifetime=7",
        ])?;
        Ok(())
    }

    fn delete(&self, cloud: &Cloud, files: &[String]) -> anyhow::Result<()> {
        let mut refused = None;
        if let Some((account, _)) = self.reads_only(&cloud.name)? {
            refused = self.delete_through(&account, cloud, files)?;
            if refused.is_none() {
                return Ok(());
            }
        }
        let remote = cloud.remote();
        if !self.run_shown(&["delete", &remote], files)? {
            let mut err = anyhow::anyhow!("some files weren't deleted from {} (above)", cloud.name);
            if let Some(said) = refused {
                err = err.context(format!("its account reads only, and refused: {said:#}"));
            }
            return Err(err);
        }
        Ok(())
    }

    fn link(&self, cloud: &Cloud, file: &str, expires: Duration) -> anyhow::Result<String> {
        ensure!(
            !cloud.encrypted,
            "{} is encrypted: a link would hand out what only its password reads",
            cloud.name
        );
        let expire = format!("{}s", expires.as_secs());
        // AWS's SDK otherwise signs `x-amz-checksum-mode` into the URL, a
        // header no browser sends: S3 then refuses every visitor.
        let output = self
            .command(&["link", "--expire", &expire, &object(cloud, file)])?
            .env("AWS_RESPONSE_CHECKSUM_VALIDATION", "when_required")
            .stdin(Stdio::null())
            .output()
            .within("running rclone")?;
        ensure!(output.status.success(), "{}", said(&output));
        let stdout = String::from_utf8_lossy(&output.stdout);
        let url = stdout.lines().rev().map(str::trim).find(|l| !l.is_empty());
        match url {
            Some(url) if url.starts_with("https://") || url.starts_with("http://") => {
                Ok(url.to_string())
            }
            _ => bail!("rclone gave no link for {file} in {}", cloud.name),
        }
    }
}
