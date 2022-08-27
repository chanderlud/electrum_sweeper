use std::collections::HashMap;
use std::fs::{File, remove_file};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

use log::{info, warn};
use serde_json::from_str;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt()]
struct Opt {
    #[structopt(short = "q", long = "quiet")]
    quiet: bool,
    #[structopt(short = "v", long = "verbose", parse(from_occurrences))]
    verbose: usize,
    #[structopt(short = "d", long = "delay")]
    delay: u64,
    #[structopt(short = "p", long = "path")]
    path: String,
    #[structopt(short = "k", long = "key_file")]
    key_file: String
}

enum ElectrumCommand {
    StartDaemon,
    Restore(String),
    LoadWallet,
    ListFundedKeys,
    Sweep((String, String)),
}

impl ElectrumCommand {
    fn run(self, path: &str) -> String {
        let command_str = match self {
            Self::StartDaemon => String::from("%path% daemon -d"),
            Self::ListFundedKeys => String::from("%path% -w sweeper_wallet listaddresses --funded | %path% -w sweeper_wallet getprivatekeys -"),
            Self::Restore(keys) => format!("%path% -w sweeper_wallet restore \"{}\"", keys),
            Self::LoadWallet => String::from("%path% -w sweeper_wallet load_wallet"),
            Self::Sweep((private_key, target)) => format!("%path% -w sweeper_wallet sweep {} {} | %path% -w sweeper_wallet broadcast -", private_key, target),
        };

        let full_command = command_str.replace("%path%", path);

        let o = Command::new("sh")
            .arg("-c")
            .arg(full_command)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn().unwrap();

        let mut s = String::new();
        let _ = o.stdout.unwrap().read_to_string(&mut s);

        s
    }
}

fn main() {
    let opt = Opt::from_args();

    stderrlog::new()
        .module(module_path!())
        .quiet(opt.quiet)
        .verbosity(opt.verbose)
        .init().unwrap();

    info!("using electrum binary: {}", opt.path);

    let wallet_path = Path::new("sweeper_wallet");
    if wallet_path.exists() {
        match remove_file(wallet_path) {
            Ok(_) => info!("removed old wallet"),
            Err(e) => warn!("failed to remove old wallet: {}", e)
        }
    }

    let mut key_data = String::new();

    let _ = File::open(&opt.key_file)
        .unwrap()
        .read_to_string(&mut key_data)
        .unwrap();

    let mut keys: HashMap<String, String> = HashMap::new();
    let mut just_keys: Vec<&str> = vec![];

    let _ = key_data
        .lines()
        .map(|v| {
            let s = v.split("|").collect::<Vec<&str>>();
            let private_key = s[0];
            let target_address = s[1];

            just_keys.push(private_key);
            keys.insert(String::from(private_key), String::from(target_address));
        }).collect::<Vec<_>>();

    let key_string = just_keys.join(" ");

    info!("loaded {} private keys", just_keys.len());

    ElectrumCommand::StartDaemon.run(&opt.path);
    info!("started electrum daemon");
    ElectrumCommand::Restore(key_string).run(&opt.path);
    ElectrumCommand::LoadWallet.run(&opt.path);
    info!("loaded electrum wallet");

    loop {
        info!("checking wallet balances");
        let output = ElectrumCommand::ListFundedKeys.run(&opt.path);
        let funded_keys: Vec<&str> = from_str(&output).unwrap();

        if funded_keys.len() > 0 {
            info!("sweeping {} keys", funded_keys.len());
        } else {
            info!("no keys with balance");
        }

        for key in funded_keys {
            let target_address = keys.get(key).unwrap();

            ElectrumCommand::Sweep((String::from(key), String::from(target_address))).run(&opt.path);

            info!("swept {} to {}", key, target_address);
        }

        sleep(Duration::from_secs(opt.delay))
    }
}
