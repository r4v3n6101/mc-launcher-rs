use std::{collections::HashMap, ffi::OsStr, sync::Arc};

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
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
    #[clap(long, short)]
    force_download: bool,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = Client::default();

    let manifest = fetch_manifest(&client).await?;
    let latest_release = manifest.latest_release().expect("latest not found");
    let file_hierarchy = Hierarchy::with_default_structure(&latest_release.id);

    println!("Fetching gamefiles index...");
    let mut repository =
        Repository::fetch(client, &file_hierarchy, latest_release.url.clone()).await?;
    println!("Tracking indices to download...");
    if args.force_download {
        repository.track_all();
    } else {
        repository.track_invalid().await?;
    }
    let repository = Arc::new(repository);

    let pb = ProgressBar::new(repository.tracked_size());
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes}",
        )
        .unwrap()
        .with_key("eta", |state| format!("{:.1}s", state.eta().as_secs_f64()))
        .progress_chars("#>-"),
    );

    let pull_task = {
        let this = Arc::clone(&repository);
        task::spawn(async move { this.pull_tracked(args.concurrency).await })
    };

    let update_pb_task = {
        let this = Arc::clone(&repository);
        let pb = pb.clone();
        task::spawn_blocking(move || {
            while this.downloader().downloaded_bytes() < this.tracked_size() {
                pb.set_position(this.downloader().downloaded_bytes());
            }
        })
    };

    pull_task.await??;
    update_pb_task.await?;

    pb.finish_and_clear();

    let features = HashMap::new();
    let command = GameCommand::new(
        file_hierarchy.gamedir.as_path(),
        OsStr::new("java"),
        repository.version_info(),
        &features,
    );
    let command = command.build_with_default_params(
        &file_hierarchy,
        repository.version_info(),
        &args.username,
    );
    println!("Game command: {:?}", command);

    Command::from(command).spawn()?.wait().await?;
    Ok(())
}
