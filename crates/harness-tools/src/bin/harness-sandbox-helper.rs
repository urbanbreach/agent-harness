#[cfg(target_os = "linux")]
fn main() {
    std::process::exit(run());
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("harness-sandbox-helper is Linux-only");
    std::process::exit(125);
}

#[cfg(target_os = "linux")]
#[allow(
    unsafe_code,
    reason = "the post-exec helper takes ownership of its inherited control socket and closes its FD allowlist"
)]
fn run() -> i32 {
    use std::ffi::CString;
    use std::fs;
    use std::io::{BufRead, BufReader};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::raw::c_char;
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixStream;

    use harness_core::sandbox::{apply_landlock_fs_plan, SandboxFsPlan};
    use rustix::io::{fcntl_setfd, FdFlags};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize)]
    #[serde(tag = "status", rename_all = "snake_case")]
    enum Frame<'a> {
        Ready,
        Error { code: &'a str, message: &'a str },
    }

    #[derive(Deserialize)]
    struct Request {
        plan: SandboxFsPlan,
    }

    struct ExecRequest {
        program: CString,
        argv: Vec<CString>,
        argv_pointers: Vec<*const c_char>,
        environment: Vec<CString>,
        environment_pointers: Vec<*const c_char>,
    }

    impl ExecRequest {
        fn new(program: String, args: Vec<String>) -> Result<Self, String> {
            let program = CString::new(program).map_err(|error| error.to_string())?;
            let mut argv = Vec::with_capacity(args.len() + 1);
            argv.push(program.clone());
            for arg in args {
                argv.push(CString::new(arg).map_err(|error| error.to_string())?);
            }
            let mut argv_pointers = argv.iter().map(|arg| arg.as_ptr()).collect::<Vec<_>>();
            argv_pointers.push(std::ptr::null());
            let environment = std::env::vars()
                .map(|(key, value)| CString::new(format!("{key}={value}")))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            let mut environment_pointers = environment
                .iter()
                .map(|entry| entry.as_ptr())
                .collect::<Vec<_>>();
            environment_pointers.push(std::ptr::null());
            Ok(Self {
                program,
                argv,
                argv_pointers,
                environment,
                environment_pointers,
            })
        }

        fn exec(&self) -> std::io::Error {
            // SAFETY: [Category 8 — FFI boundary]
            // Every C string and null-terminated pointer vector was allocated before seccomp and
            // remains owned by `self` for this non-returning execve call. The kernel either
            // atomically replaces this process or returns errno without retaining the pointers.
            let result = unsafe {
                execve(
                    self.program.as_ptr(),
                    self.argv_pointers.as_ptr(),
                    self.environment_pointers.as_ptr(),
                )
            };
            debug_assert_eq!(result, -1);
            std::io::Error::last_os_error()
        }
    }

    unsafe extern "C" {
        fn write(fd: i32, buffer: *const u8, count: usize) -> isize;
        fn syscall(number: isize, first: u32, last: u32, flags: u32) -> isize;
        fn execve(
            filename: *const c_char,
            argv: *const *const c_char,
            envp: *const *const c_char,
        ) -> i32;
    }

    fn write_frame(control: &UnixStream, frame: &Frame<'_>) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(frame).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        let mut remaining = bytes.as_slice();
        while !remaining.is_empty() {
            // SAFETY: [Category 8 — FFI boundary]
            // `remaining` is a live byte slice for this synchronous write, and the helper owns
            // the control socket. Raw write is used because the installed seccomp filter denies
            // socket send operations before READY while allowing this one-way protocol write.
            let written =
                unsafe { write(control.as_raw_fd(), remaining.as_ptr(), remaining.len()) };
            if written < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let written = usize::try_from(written).map_err(|error| error.to_string())?;
            if written == 0 {
                return Err("control frame write made no progress".to_string());
            }
            remaining = &remaining[written..];
        }
        Ok(())
    }

    fn parse_arguments() -> Result<(i32, String, Vec<String>), String> {
        let mut args = std::env::args();
        let _binary = args.next();
        if args.next().as_deref() != Some("--control-fd") {
            return Err("missing --control-fd".to_string());
        }
        let fd = args
            .next()
            .ok_or_else(|| "missing control fd value".to_string())?
            .parse::<i32>()
            .map_err(|error| format!("invalid control fd: {error}"))?;
        if args.next().as_deref() != Some("--") {
            return Err("missing command separator".to_string());
        }
        let program = args
            .next()
            .ok_or_else(|| "missing sandbox command".to_string())?;
        Ok((fd, program, args.collect()))
    }

    fn reject_socket_standard_io() -> Result<(), String> {
        for fd in [0, 1, 2] {
            let path = format!("/proc/self/fd/{fd}");
            let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
            if metadata.file_type().is_socket() {
                return Err(format!(
                    "fd{fd} is a socket; refusing ambiguous standard-I/O contract"
                ));
            }
        }
        Ok(())
    }

    fn close_unallowlisted_fds(control_fd: i32) -> Result<(), String> {
        const SYS_CLOSE_RANGE: isize = 436;
        let control_fd = u32::try_from(control_fd).map_err(|error| error.to_string())?;
        let close_range = |first, last| {
            // SAFETY: [Category 8 — FFI boundary]
            // Linux `close_range` only receives integer bounds. This helper is post-exec and
            // single-threaded, so no concurrent code can allocate an FD between allowlist
            // calculation and closure. The ranges exclude fd0/1/2 and the control socket.
            let result = unsafe { syscall(SYS_CLOSE_RANGE, first, last, 0) };
            if result == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error().to_string())
            }
        };
        if control_fd > 3 {
            close_range(3, control_fd - 1)?;
        }
        close_range(control_fd.saturating_add(1), u32::MAX)?;
        Ok(())
    }

    let (control_fd, program, args) = match parse_arguments() {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("invalid sandbox helper invocation: {error}");
            return 125;
        }
    };
    // SAFETY: [Category 13 — FromRawFd contract]
    // The parent passes one inherited Unix-domain control socket as `--control-fd`; ownership
    // transfers exactly once to this helper after exec.
    let control = unsafe { UnixStream::from_raw_fd(control_fd) };
    let result = (|| -> Result<(), (&str, String)> {
        let mut request = String::new();
        BufReader::new(&control)
            .read_line(&mut request)
            .map_err(|error| ("invalid_request", error.to_string()))?;
        let Request { plan } = serde_json::from_str(&request)
            .map_err(|error| ("invalid_request", error.to_string()))?;
        let exec_request = ExecRequest::new(program, args).map_err(|error| ("command", error))?;
        fcntl_setfd(&control, FdFlags::CLOEXEC)
            .map_err(|error| ("fd_closure", error.to_string()))?;
        reject_socket_standard_io().map_err(|error| ("unsafe_standard_io", error))?;
        close_unallowlisted_fds(control.as_raw_fd()).map_err(|error| ("fd_closure", error))?;
        apply_landlock_fs_plan(&plan).map_err(|error| ("restriction", error))?;
        write_frame(&control, &Frame::Ready).map_err(|error| ("command", error))?;
        Err(("command", exec_request.exec().to_string()))
    })();
    match result {
        Ok(()) => 0,
        Err((code, message)) => {
            let _ = write_frame(
                &control,
                &Frame::Error {
                    code,
                    message: &message,
                },
            );
            125
        }
    }
}
