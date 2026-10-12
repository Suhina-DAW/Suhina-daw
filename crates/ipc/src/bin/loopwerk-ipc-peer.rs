//! A stand-in for the plug-in host, used by the `ipc` tests: it opens the link named on its
//! command line and serves blocks, halving every sample.
//!
//! `loopwerk-ipc-peer <address> <mode>`, where mode is `serve` (until no block comes for two
//! seconds), `crash` (exits in the middle of block 4, without answering) or `hang` (stops
//! answering in the middle of block 4).

use std::time::Duration;

use loopwerk_ipc::Served;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(address), Some(mode)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: loopwerk-ipc-peer <address> <serve|crash|hang>");
        std::process::exit(2);
    };
    let mut served = match Served::open(address) {
        Ok(served) => served,
        Err(e) => {
            eprintln!("loopwerk-ipc-peer: {e}");
            std::process::exit(1);
        }
    };
    let mut block = 0u32;
    loop {
        let fails_now = block == 4 && mode != "serve";
        let served_one = served.serve(Duration::from_secs(2), |input, output| {
            if fails_now {
                if mode == "crash" {
                    std::process::exit(3);
                }
                loop {
                    std::thread::sleep(Duration::from_secs(60));
                }
            }
            for (o, i) in output.iter_mut().zip(input) {
                *o = i * 0.5;
            }
        });
        if !served_one {
            return;
        }
        block += 1;
    }
}
