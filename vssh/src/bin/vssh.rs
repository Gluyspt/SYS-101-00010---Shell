use std::env;
use std::path::Path;
use std::io::{self, Write};
use std::ffi::CString;
use nix::{sys::wait::waitpid};
use nix::unistd::{fork, ForkResult, execvp, dup2, pipe};
use std::fs::File;
use std::os::unix::io::{AsRawFd, IntoRawFd};

fn externalize(args: &[&str]) -> Vec<CString> {
    args.iter()
        .map(|&s| CString::new(s).unwrap())
        .collect()
}

fn main() -> std::io::Result<()> {
    loop {
        let path = env::current_dir()?;
        print!("{}$ ", path.display());
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let mut cmd: Vec<&str> = input.trim().split_whitespace().collect();
        
        if cmd.is_empty() { 
            continue; 
        }

        let background: bool;
        if cmd.last() == Some(&"&"){
            cmd.pop();
            background = true;
        } else {
            background = false;
        }

        if cmd.is_empty() {
            continue;
        }

        let command = cmd[0];
        let args = &cmd[1..];
        
        match command {
            "exit" => break,
            "cd" => {
                if let Some(target_directory) = args.get(0) {
                    let path = Path::new(target_directory);
                    
                    if let Err(e) = std::env::set_current_dir(path){
                        eprintln!("Error changing directory: {}", path.display());
                    }
                }   else {
                        eprintln!("cd: missing argument");
                }
            
            },
            
            _ => {
                let stages: Vec<&str> = input.trim().split('|').collect();
                let mut prev_pipe_read: Option<i32> = None;
                let mut child_pids = Vec::new();
                
                for (idx, stage) in stages.iter().enumerate() {
                    let is_first = idx == 0;
                    let is_last = idx == stages.len() - 1;
                    
                    let mut tokens: Vec<&str> = stage.split_whitespace().collect();
                    if tokens.is_empty() { 
                        continue; 
                    }
                    
                    if is_last && tokens.last() == Some(&"&") {
                        tokens.pop();
                    }

                    let mut curr_pipe: Option<(i32, i32)> = None;
                    if !is_last {
                        if let Ok((r, w)) = pipe() {
                            curr_pipe = Some((r.into_raw_fd(), w.into_raw_fd()));
                        }
                    }

                    match unsafe { fork() } {
                        Ok(ForkResult::Parent { child, .. }) => {
                            child_pids.push(child);
                            if let Some((_, w)) = curr_pipe { unsafe { nix::libc::close(w); } }
                            if let Some(r) = prev_pipe_read { unsafe { nix::libc::close(r); } }
                            prev_pipe_read = curr_pipe.map(|(r, _)| r);
                        }
                        Ok(ForkResult::Child) => {
                            if let Some(r_fd) = prev_pipe_read { 
                                let _ = dup2(r_fd, 0); 
                            }
                            if let Some((_, w_fd)) = curr_pipe { 
                                let _ = dup2(w_fd, 1); 
                            }

                            let mut final_args = Vec::new();
                            let mut i = 0;
                            
                            while i < tokens.len() {
                                match tokens[i] {
                                    "<" if is_first => {
                                        let file = File::open(tokens[i+1]).expect("Input file error");
                                        let _ = dup2(file.as_raw_fd(), 0);
                                        i += 2;
                                    }
                                    ">" if is_last => {
                                        let file = File::create(tokens[i+1]).expect("Output file error");
                                        let _ = dup2(file.as_raw_fd(), 1);
                                        i += 2;
                                    }
                                    _ => {
                                        final_args.push(tokens[i]);
                                        i += 1;
                                    }
                                }
                            }

                            if let Some((r, w)) = curr_pipe { 
                                unsafe { nix::libc::close(r); nix::libc::close(w); } 
                            }

                            let c_args = externalize(&final_args);
                            let _ = execvp(&c_args[0], &c_args);

                            std::process::exit(1);
                        }
                        Err(e) => eprintln!("Fork failed: {}", e),
                    }
                }

                if !background {
                    for pid in child_pids {
                        let _ = waitpid(pid, None);
                    }
                } else if let Some(&last_pid) = child_pids.last() {
                    println!("[PID] {}", last_pid);
                }
            }
        }
    }
    Ok(())
}
