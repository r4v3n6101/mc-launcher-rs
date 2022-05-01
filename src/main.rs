use std::{collections::HashMap, ffi::OsStr, sync::Arc};

use clap::Parser;
use indicatif::{HumanBytes, ProgressBar, ProgressStyle};
use mcl_rs::{
    io::{file::Hierarchy, sync::Repository},
    process::GameCommand,
    resources::fetch_manifest,
};
use reqwest::Client;
use tokio::{process::Command, task};

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    #[clap(short, long, default_value = "test")]
    username: String,
    #[clap(long, default_value = "256")]
    concurrency: usize,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = Client::default();

    let manifest = fetch_manifest(&client).await?;
    let latest_release = manifest.latest_release().expect("latest not found");
    let file_hierarchy = Hierarchy::with_default_structure(&latest_release.id);

    println!("Fetching gamefiles index...");
    let repository = Arc::new(Repository::fetch(client, file_hierarchy, latest_release).await?);
    let pb = ProgressBar::new(repository.indices() as u64);
    let ps = ProgressStyle::with_template(
        "[{elapsed_precise}] {wide_bar:80.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap()
    .progress_chars("##-");
    pb.set_style(ps);

    let pull_task = {
        let this = Arc::clone(&repository);
        task::spawn(async move { this.pull(args.concurrency).await })
    };

    let update_pb_task = {
        let this = Arc::clone(&repository);
        let pb = pb.clone();
        task::spawn_blocking(move || {
            let mut prev = this.pulled_indices();
            loop {
                if this.pulled_indices() >= this.indices() {
                    break;
                }
                if prev != this.pulled_indices() {
                    prev = this.pulled_indices();

                    pb.set_position(this.pulled_indices() as u64);
                    pb.set_message(HumanBytes(this.downloader().downloaded_bytes()).to_string());
                }
            }
        })
    };

    pull_task.await??;
    update_pb_task.await?;

    pb.finish();

    let features = HashMap::new();
    let command = GameCommand::new(
        repository.hierarchy().gamedir.as_path(),
        OsStr::new("java"),
        repository.version_info(),
        &features,
    );
    let command = command.build_with_default_params(
        repository.hierarchy(),
        repository.version_info(),
        &args.username,
    );
    println!("Game command: {:?}", command);

    Command::from(command).spawn()?.wait().await?;
    Ok(())
}
