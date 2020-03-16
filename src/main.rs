mod cfg;
mod client;
mod runner;

use anyhow::{bail, Context, Result};
use cfg::Config;
use client::Client;

fn load_cfg() -> Result<Config> {
    let cfg_path = match std::env::args_os().nth(1) {
        Some(p) => p,
        None => bail!("usage: <path/to/config.yaml>"),
    };
    let cfg_data = std::fs::read(cfg_path).context("file is not readable")?;

    serde_yaml::from_slice(&cfg_data).context("parse error")
}
struct State {
    clients: Vec<Client>,
}
fn main() -> Result<()> {
    if std::env::var("__RUN__").is_ok() {
        return runner::runner_main()
    }
    println!("loading config");
    let config = load_cfg().context("failed to load config")?;
    println!("Spawning clients");
    let mut clients = vec![];
    for program_path in &config.programs {
        clients.push(
            client::Client::new(program_path, config.image.as_deref())
                .context("internal error when spawning bot")?,
        );
    }
    let mut score = vec![0; clients.len()];
    let mut state = State { clients };
    wait_ready(&mut state);
    for i in 0..config.rounds {
        println!("Round #{}", i);
        let outcome = play_round(&mut state);
        match outcome.winners.get(0) {
            Some(&winner) => {
                println!("winner is client #{}", winner);
                score[winner] += 1;
            }
            None => {
                println!("All bots loosed");
            }
        }
    }
    for i in 0..state.clients.len() {
        state.clients[i].send_end();
        println!(
            "Client #{} ({}) - {} points",
            i,
            state.clients[i].name(),
            score[i]
        );
    }
    Ok(())
}

fn wait_ready(state: &mut State) {
    println!("waiting for readiness");
    for client in &mut state.clients {
        client.poll();
        if client.is_init() {
            println!("client {} still initializing", client);
        }
    }
    println!("wait done");
}
#[derive(Debug)]
struct RoundOutcome {
    winners: Vec<usize>,
}

fn play_round(state: &mut State) -> RoundOutcome {
    let mut nums = vec![];
    for client in &mut state.clients {
        client.send_game();
        client.poll();
        let num = client.get_num();
        nums.push(num);
    }
    for client in &mut state.clients {
        client.send_nums(&nums);
    }
    let mut set_used = std::collections::HashSet::new();
    let mut set_loose = std::collections::HashSet::new();
    for x in &nums {
        if !set_used.insert(x) {
            set_loose.insert(x);
        }
    }
    let mut winners: Vec<_> = nums
        .iter()
        .enumerate()
        .filter(|(_pos, val)| !set_loose.contains(val))
        .collect();
    winners.sort_by_key(|(_pos, val)| *val);
    let winners = winners.into_iter().map(|(pos, _val)| pos).collect();
    RoundOutcome { winners }
}
