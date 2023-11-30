use std::collections::HashMap;

use nix::{unistd, sched, sys::{signal::Signal, wait::WaitStatus}, mount::{self, MsFlags}};

use nix::sched::CloneCb;
use anyhow::Result;

use crate::image::Image;


#[derive(Debug)]
pub(crate) struct Container {
    arguments: Vec<String>,
    mounts: Vec<Mount>,
    image: Option<Image>,
    fs_root: String
}

#[derive(Debug)]
struct Mount {
    name: String
}

// Need to implement Drop such that we can umount it.
impl Mount {
    fn unmount(&self) {
        mount::umount(self.name.as_str()).unwrap();
    }
}

impl Container {

    pub(crate) fn new(container_arguments: &[String], fs_root: &str) -> Result<Self> {

        let image_str = &container_arguments[0];

        let image_parts: Vec<_> = image_str.split(':').collect();

        let image = match image_parts[..] {
            [img] => Image::pull(img, "latest", fs_root),
            [img, tag] => Image::pull(img, tag, fs_root),
            _ => anyhow::bail!("{:?} was not in a correct format of image:tag or just image", image_parts)
        }?;

        Ok(Self {
            arguments: container_arguments[1..].to_vec(),
            mounts: vec![],
            image: Some(image),
            fs_root: fs_root.to_string()
        })
    }

    pub(crate) fn run(mut self) -> CloneCb<'static> {
        Box::new(move || self.run_inner())
    }

    fn run_inner(&mut self) -> isize {

        println!("Setting up jail");

        // Filesystem!
        // Change root filesystem
        unistd::chroot(self.fs_root.as_str()).expect("could not change chroot dir");
        // Move to '/' since our current dir is now unreachable, which gives Undefined Behaivour.
        unistd::chdir("/").expect("could not change dir");

        // Hostname
        unistd::sethostname("lcontainer").expect("Could not set hostname");

        // Mount proc
        mount::mount(Some("proc"), "proc", Some("proc"), MsFlags::empty(), None::<&str>).expect("failed to mount proc");
        self.mounts.push(Mount { name: String::from("proc") });

        // If not cmd provided, get arguments from image config

        let image_config = self.image.as_ref().expect("image_config").config();
        let entry = image_config.config.entrypoint.as_ref().map(|x| x.first());
        let args = image_config.config.cmd.clone();
        let envs: HashMap<String, String> = image_config.config.env.iter().map(|s| {
            let (key, val) = s.split_once('=').expect("split env");
            (key.to_string(), val.to_string())
        }).collect();

        let mut exit = match self.arguments.len() {
            0 => {
                let entry = entry.expect("entry1").expect("entry2");
                std::process::Command::new(entry)
                    .args(&args)
                    .env_clear()
                    .envs(envs)
                    .spawn()
                    .expect("Process failed")      
            },
            _ => {
                std::process::Command::new(&self.arguments[0])
                    .args(&self.arguments[1..])
                    .env_clear()
                    .envs(envs)
                    .spawn()
                    .expect("Process failed")      
            }
        };

        let exit_code = exit.wait();
        for mount in self.mounts.iter() {
            mount.unmount();
        }

        
        match exit_code.map(|e| e.code()).ok() {
            Some(Some(code)) => code as isize,
            _ => -1
        }

        // We're currently running as PID 1, if we want to replace our process with another, which is running PID 1 use the code below:
        // Switch process to execute the provided command as PID 1
        // use std::ffi::CString;
        // let command = CString::new(self.arguments[1].as_bytes()).unwrap();
        // let arguments: Vec<_> = self.arguments[1..].iter().map(|s| CString::new(s.as_bytes()).unwrap()).collect();
        // unistd::execv(command.as_c_str(), arguments.as_slice()).expect(format!("failed {:?} -- {:?}", &command, &arguments).as_str());
        // unreachable!()
    }

    pub(crate) fn spawn(arguments: &[String], root_fs: &str) -> Result<()> {
        
        const STACK_SIZE: usize = 1024 * 1024;
        let child_process_stack: &mut [u8; STACK_SIZE] = &mut [0; STACK_SIZE];
    
        let process_clone_flags = sched::CloneFlags::CLONE_NEWUTS 
            | sched::CloneFlags::CLONE_NEWPID;
            // | sched::CloneFlags::CLONE_NEWCGROUP 
            // | sched::CloneFlags::CLONE_NEWNS
            // | sched::CloneFlags::CLONE_NEWIPC 
            // | sched::CloneFlags::CLONE_NEWNET; 
        
        let child_container = Container::new(arguments, root_fs)?;
        
        let child_pid = unsafe {
            sched::clone(child_container.run(), child_process_stack, process_clone_flags, Some(Signal::SIGCHLD as i32)).expect("expected to create process child")
        };
        
        // Lets wait for the process to have exited.        
        let wait_status = nix::sys::wait::waitpid(child_pid, None).expect("waitpid failed");
        
        match wait_status {
            WaitStatus::Exited(_, exit_code) => {
                println!("exited with code {}", exit_code);
                std::process::exit(exit_code)
            },
            _ => {
                println!("got an unknown wait status: {:?}", wait_status);
                std::process::exit(-1)
            }
            // WaitStatus::Signaled(_, _, _) => todo!(),
            // WaitStatus::Stopped(_, _) => todo!(),
            // WaitStatus::PtraceEvent(_, _, _) => todo!(),
            // WaitStatus::PtraceSyscall(_) => todo!(),
            // WaitStatus::Continued(_) => todo!(),
            // WaitStatus::StillAlive => todo!(),
        }
    }
}

