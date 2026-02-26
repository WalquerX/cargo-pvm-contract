use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

/// Each test gets its own anvil-polkadot instance on a unique port.
/// `Drop` kills the process when the test ends — no shared state between tests.
static NEXT_PORT: AtomicU16 = AtomicU16::new(19545);

pub struct AnvilPolkadot {
    pub rpc_url: String,
    child: Child,
}

impl AnvilPolkadot {
    pub fn start() -> Self {
        let port = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
        let child = Command::new("anvil-polkadot")
            .args(["--port", &port.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("anvil-polkadot must be installed. Install:\n\
                     curl -L https://raw.githubusercontent.com/paritytech/foundry-polkadot/refs/heads/master/foundryup/install | bash\n\
                     foundryup-polkadot");

        let rpc_url = format!("http://127.0.0.1:{port}");

        let start = Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(30) {
                panic!("anvil-polkadot failed to start on port {port} within 30 seconds");
            }
            if let Ok(output) = Command::new("cast")
                .args(["block-number", "--rpc-url", &rpc_url])
                .output()
                && output.status.success()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }

        Self { rpc_url, child }
    }
}

impl Drop for AnvilPolkadot {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
