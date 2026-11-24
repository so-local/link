//! `lnk-aws`: the Amazon Web Services adapter. `lnk cloud` and `lnk box`
//! run it (the contract is `link_cloud_contract`); nobody types it.

mod boxes;
mod buckets;
mod cli;
mod connect;
mod images;
mod install;
mod prices;

use link_cloud_contract::{self as cloud, Reply};
use link_format::{Done, Exit, Foreign, Out, Product, Report, WithinExt, sentence};

fn main() -> std::process::ExitCode {
    // Safe to hold a secret before anything else: no core dump, no other
    // process of this user reading it.
    link_secretive::process::guard();
    link_core::describe_component!();
    link_settings::output::run(|out| {
        // Run by `lnk cloud`, `lnk box` and `lnk storage`, which say what
        // to do next: a failure here is the cloud's or the person's, never
        // Link's own, and says only what went wrong.
        try_main(out).map_err(|mut report| {
            report.adopted = false;
            report
        })
    })
}

/// The commands of the cloud adapter contract it answers on stdin, beside
/// `connect` and `tunnel`.
fn try_main(out: &mut Out) -> Result<Done, Report> {
    let parsed = link_aws::flags::parse().command.command;

    // `lnk uninstall` and `lnk module remove` (`link_core::uninstall`).
    if let link_aws::flags::Command::Uninstall(given) = &parsed {
        let args = given;
        use link_core::uninstall as u;
        let kept = u::Kept {
            parts: vec![
            u::Part {
                name: "clouds".into(),
                about: "your cloud accounts' keys, and Link's SSH key to your boxes (the boxes keep running, and billing)".into(),
                paths: vec![u::settings(link_files::layout::Hidden::Aws)?],
                keys: vec![],
                ..u::Part::default()
            },
            ],
            ..u::Kept::default()
        };
        u::run(args, &kept, || Ok(()), |_| Ok(()))?;
        return Ok(Done::shown());
    }
    if let link_aws::flags::Command::AwsConnect(given) = parsed {
        let account = connect::connect(out, given)?;
        cloud::reply(&account)?;
        return Ok(Done::shown());
    }
    // A box's SSH over Systems Manager: stdin and stdout are the tunnel's.
    if let link_aws::flags::Command::AwsTunnel(given) = &parsed {
        let named = given
            .cloud
            .as_deref()
            .within("a tunnel names its cloud account, --cloud <cloud>")?;
        let tunnel = cloud::Tunnel::new(named, &given.zone, &given.id)?;
        let account = cloud::account(&tunnel.cloud)
            .within(format!("no cloud account named {:?}", tunnel.cloud))?;
        let aws = cli::Aws::new(&account, Some(&tunnel.zone))?;
        // ssh's own bytes flow through stdin and stdout.
        let _ = out.hand_over();
        aws.cli.carry(&[
            "ssm",
            "start-session",
            &format!("--target={}", tunnel.id),
            "--document-name",
            "AWS-StartSSHSession",
            "--parameters",
            "portNumber=22",
        ])?;
        return Ok(Done::shown());
    }
    let command = parsed.word();
    let request: cloud::Request = cloud::read_request()?;
    // The account's own: no AWS CLI needed.
    match command {
        "storage" => {
            cloud::reply(&connect::storage(&request.account)?)?;
            return Ok(Done::shown());
        }
        "disconnect" => {
            cloud::reply(&Reply::default())?;
            return Ok(Done::shown());
        }
        _ => {}
    }
    let aws = cli::Aws::new(&request.account, request.place.as_deref())?;
    let id = || request.id.as_deref().within("no box id in the request");
    let bucket = || request.bucket.as_deref().within("no bucket in the request");
    let reply = match command {
        "defaults" => {
            if let Some(spec) = &request.spec {
                aws.attachment_args(spec.attach())?;
            }
            Reply {
                attachments: request.spec.as_ref().map(|_| true),
                place: Some(aws.region.clone()),
                size: Some("t3.small".into()),
                ..Reply::default()
            }
        }
        "sizes" => {
            let mut sizes = aws.sizes()?;
            // Sizes without their prices when AWS gives none here (the
            // account may not read them): none, never a guess.
            let prices = aws.prices().ok();
            if let Some(prices) = &prices {
                for size in &mut sizes {
                    size.price = prices.sizes.get(&size.size).copied();
                }
            }
            Reply {
                sizes: cloud::carried(sizes),
                rates: prices.map(|p| p.rates),
                ..Reply::default()
            }
        }
        "launch" => Reply {
            id: Some(aws.launch(
                request.spec.as_ref().within("no spec in the request")?,
                request.image.as_deref(),
            )?),
            ..Reply::default()
        },
        "wait" => Reply {
            ip: Some(aws.wait_running(id()?)?),
            ..Reply::default()
        },
        "start" => Reply {
            ip: Some(aws.start(id()?)?),
            ..Reply::default()
        },
        "stop" => {
            aws.stop(id()?)?;
            Reply::default()
        }
        "state" => {
            let (state, stopped_by_cloud) = aws.state_and_why(id()?)?;
            Reply {
                state: Some(state),
                stopped_by_cloud,
                ..Reply::default()
            }
        }
        "describe" => Reply {
            boxes: Some(vec![aws.describe_box(id()?)?]),
            ..Reply::default()
        },
        "remove" => {
            aws.remove(id()?)?;
            Reply::default()
        }
        "hostkeys" => Reply {
            host_keys: cloud::carried(aws.host_keys(id()?)?),
            ..Reply::default()
        },
        "cleanup" => {
            aws.cleanup()?;
            Reply::default()
        }
        "save-hostkeys" => {
            aws.save_host_keys(id()?, request.host_keys())?;
            Reply::default()
        }
        "add-key" => {
            let key = request
                .public_key
                .as_deref()
                .within("no key in the request")?;
            aws.add_key(id()?, key)?;
            Reply {
                temporary: Some(true),
                ..Reply::default()
            }
        }
        "remove-keys" => {
            aws.remove_keys(id()?)?;
            Reply::default()
        }
        "tag" => {
            aws.tag(id()?, &request.tags)?;
            Reply::default()
        }
        "list" => {
            let (boxes, denied) = aws.list(request.owner.as_deref(), request.places())?;
            Reply {
                boxes: cloud::carried(boxes),
                denied: cloud::carried(denied),
                ..Reply::default()
            }
        }
        "label" => {
            aws.label(bucket()?, &request.labels)?;
            Reply::default()
        }
        "buckets" => Reply {
            buckets: cloud::carried(aws.buckets(&request.labels)?),
            ..Reply::default()
        },
        "upload-once" => Reply {
            upload: Some(
                aws.upload_once(
                    bucket()?,
                    request
                        .object
                        .as_deref()
                        .within("no object in the request")?,
                    request.expires.within("no upload lifetime")?,
                )?,
            ),
            ..Reply::default()
        },
        "object-read" => Reply {
            contents: aws.object_read(
                bucket()?,
                request
                    .object
                    .as_deref()
                    .within("no object in the request")?,
            )?,
            ..Reply::default()
        },
        "bucket-credential" => Reply {
            credential: aws.bucket_credential(bucket()?)?,
            ..Reply::default()
        },
        "revoke-key" => {
            aws.revoke_key(bucket()?)?;
            Reply::default()
        }
        "image-save" => {
            let name = request
                .image
                .as_deref()
                .within("no image name in the request")?;
            Reply {
                images: Some(vec![aws.save_image(id()?, name, &request.tags)?]),
                ..Reply::default()
            }
        }
        "image-list" => Reply {
            images: cloud::carried(aws.images(request.owner.as_deref(), request.places())?),
            ..Reply::default()
        },
        "image-remove" => {
            let image = request.image.as_deref().within("no image in the request")?;
            let owner = request.owner.as_deref().within("no owner in the request")?;
            aws.remove_image(image, owner)?;
            Reply::default()
        }
        other => return Err(no_command(other)),
    };
    cloud::reply(&reply)?;
    Ok(Done::shown())
}

/// What a caller reads to know the adapter lacks a command (it says "has no
/// command").
fn no_command(command: &str) -> Report {
    Report::error(sentence!(
        "{} has no command {}.",
        Product::new("lnk-aws"),
        Foreign::new(command)
    ))
    .exit(Exit::Usage)
}

#[cfg(test)]
link_test_kit::program!(unit);
