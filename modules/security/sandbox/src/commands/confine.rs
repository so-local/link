//! The hidden steps that confine a program or isolate a worker, as `wrap` starts them.

#[cfg(target_os = "linux")]
use link_format::WithinExt;
use link_format::{Done, Report};
use link_sandbox::cli::{self};

/// `sandbox confine`: confines itself, then runs its program in its place.
pub(crate) fn confine(
    cli::SandboxConfineArgs {
        only,
        exec,
        file_wall,
        read,
        write,
        argv,
        worker_user,
        worker_group,
        worker_dir,
    }: cli::SandboxConfineArgs,
) -> Result<Done, Report> {
    if let Some(uid) = worker_user {
        #[cfg(target_os = "linux")]
        {
            link_sandbox_confine::linux::workers::enter(uid, &worker_group)?;
            if let Some(dir) = worker_dir {
                std::env::set_current_dir(&dir)
                    .within(format!("entering worker folder {}", dir.display()))?;
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (uid, worker_group, worker_dir);
            return Err(anyhow::anyhow!("worker identities are Linux's").into());
        }
    }
    let wall = file_wall.then_some(link_sandbox_confine::Wall {
        read: &read,
        write: &write,
    });
    match link_sandbox_confine::here()?.enter(wall.as_ref(), only.then_some(&exec[..]), &argv)? {}
}

/// `sandbox worker`: an isolated box worker's launch.
pub(crate) fn worker(
    cli::SandboxWorkerArgs {
        plan,
        parent_fd,
        argv,
    }: cli::SandboxWorkerArgs,
) -> Result<Done, Report> {
    #[cfg(target_os = "linux")]
    {
        let code = link_sandbox_confine::linux::workers::run(&plan, &parent_fd, &argv)
            .within("launching an isolated box worker")?;
        Ok(Done::passed(code))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (plan, parent_fd, argv);
        Err(anyhow::anyhow!("worker identities are Linux's").into())
    }
}
