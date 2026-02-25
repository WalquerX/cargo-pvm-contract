use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub struct AnvilPolkadot {
    child: Child,
    pub rpc_url: String,
}

impl AnvilPolkadot {
    /// Reset anvil state to genesis (wipes all deployed contracts and nonces).
    pub fn reset(&self) {
        let output = Command::new("cast")
            .args(["rpc", "anvil_reset", "--rpc-url", &self.rpc_url])
            .output()
            .expect("cast rpc anvil_reset failed to execute");
        assert!(
            output.status.success(),
            "anvil_reset failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub fn start() -> Self {
        let child = Command::new("anvil-polkadot")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("anvil-polkadot must be installed. Install:\n\
                     curl -L https://raw.githubusercontent.com/paritytech/foundry-polkadot/refs/heads/master/foundryup/install | bash\n\
                     foundryup-polkadot");

        let rpc_url = "http://127.0.0.1:8545".to_string();

        let start = Instant::now();
        loop {
            if start.elapsed() > Duration::from_secs(30) {
                panic!("anvil-polkadot failed to start within 30 seconds");
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

        Self { child, rpc_url }
    }
}

impl Drop for AnvilPolkadot {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static ANVIL_INSTANCE: OnceLock<AnvilPolkadot> = OnceLock::new();

pub fn shared_anvil() -> &'static AnvilPolkadot {
    ANVIL_INSTANCE.get_or_init(AnvilPolkadot::start)
}
