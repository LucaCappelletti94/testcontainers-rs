//! Runs the victim in a new process group, sends it CTRL_BREAK_EVENT once its container is up,
//! and a second one a second later when asked to, then checks the exit code and that the
//! container is gone.

#[cfg(windows)]
fn main() {
    use std::{
        io::{BufRead, BufReader},
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    use windows_sys::Win32::System::Console::{
        AllocConsole, GenerateConsoleCtrlEvent, GetConsoleProcessList, CTRL_BREAK_EVENT,
    };

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const STATUS_CONTROL_C_EXIT: i32 = 0xC000_013A_u32.cast_signed();
    const START_BOUND: Duration = Duration::from_secs(600);
    const EXIT_BOUND: Duration = Duration::from_secs(120);

    fn container_listed(id: &str) -> bool {
        let out = Command::new("docker")
            .args(["ps", "-a", "-q", "--no-trunc", "--filter"])
            .arg(format!("id={id}"))
            .output()
            .expect("docker ps runs");
        assert!(out.status.success(), "docker ps failed: {out:?}");
        !String::from_utf8_lossy(&out.stdout).trim().is_empty()
    }

    let mut probe = [0u32; 1];
    // SAFETY: the buffer is valid for one element and its length is passed.
    if unsafe { GetConsoleProcessList(probe.as_mut_ptr(), 1) } == 0 {
        // SAFETY: no console is attached, so allocating one is valid.
        assert_ne!(unsafe { AllocConsole() }, 0, "AllocConsole failed");
        println!("driver: allocated a console");
    }

    let victim = std::env::args().nth(1).expect("victim path argument");
    let events: u32 = std::env::args()
        .nth(2)
        .map_or(1, |n| n.parse().expect("event count is a number"));
    let mut child = Command::new(victim)
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .stdout(Stdio::piped())
        .spawn()
        .expect("victim spawns");

    let (tx, rx) = std::sync::mpsc::channel();
    let stdout = child.stdout.take().expect("victim stdout is piped");
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.expect("victim stdout is utf-8");
            println!("victim: {line}");
            if let Some(id) = line.strip_prefix("CONTAINER ") {
                let _ = tx.send(id.to_owned());
            }
        }
    });

    let id = rx
        .recv_timeout(START_BOUND)
        .expect("victim reports its container within the bound");
    assert!(
        container_listed(&id),
        "container {id} is listed before the signal"
    );
    println!(
        "driver: container {id} is up, sending CTRL_BREAK_EVENT to group {}",
        child.id()
    );

    let sent = Instant::now();
    // SAFETY: no pointer arguments, and the id names the group made by CREATE_NEW_PROCESS_GROUP.
    let ok = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) };
    assert_ne!(
        ok,
        0,
        "GenerateConsoleCtrlEvent failed: {}",
        std::io::Error::last_os_error()
    );

    if events == 2 {
        std::thread::sleep(Duration::from_secs(1));
        if child
            .try_wait()
            .expect("victim status is readable")
            .is_none()
        {
            println!("driver: sending a second CTRL_BREAK_EVENT");
            // SAFETY: no pointer arguments, and the id names the group made by CREATE_NEW_PROCESS_GROUP.
            let ok = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, child.id()) };
            assert_ne!(
                ok,
                0,
                "GenerateConsoleCtrlEvent failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }

    let status = loop {
        if let Some(status) = child.try_wait().expect("victim status is readable") {
            break status;
        }
        if sent.elapsed() > EXIT_BOUND {
            let _ = child.kill();
            panic!("victim did not exit within {EXIT_BOUND:?} of CTRL_BREAK_EVENT");
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let elapsed = sent.elapsed();

    let code = status.code().expect("victim has an exit code");
    let listed = container_listed(&id);
    println!(
        "driver: victim exited with {:#010X} after {elapsed:?}, container listed afterwards: {listed}",
        code.cast_unsigned()
    );
    assert_eq!(
        code, STATUS_CONTROL_C_EXIT,
        "exit code is STATUS_CONTROL_C_EXIT"
    );
    assert!(!listed, "container {id} is removed");
    println!("driver: PASS");
}

#[cfg(not(windows))]
fn main() {
    panic!("the driver sends a Windows console control event and runs only on Windows");
}
