use std::collections::HashMap;

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use mcl_rs::{
    io::{download::Manager, file::Hierarchy, sync::RemoteRepository},
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
    let downloader = Manager::new(client);

    println!("Fetching gamefiles index...");
    let repository =
        RemoteRepository::fetch(&downloader, &file_hierarchy, latest_release.url.clone()).await?;
    println!("Fetched {}KB", downloader.downloaded_bytes() / 1024);
    downloader.reset();

    println!("Tracking indices to download...");
    let tracked = if args.force_download {
        repository.track_all()
    } else {
        repository.track_invalid().await?
    };

    let tracked_size = tracked.bytes_size();
    let pb = ProgressBar::new(tracked_size);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes}",
        )
        .unwrap()
        .with_key("eta", |state| format!("{:.1}s", state.eta().as_secs_f64()))
        .progress_chars("#>-"),
    );

    let pb_update_task = {
        let pb = pb.clone();
        let downloader = downloader.clone();
        task::spawn_blocking(move || {
            while downloader.downloaded_bytes() < tracked_size {
                pb.set_position(downloader.downloaded_bytes());
            }
        })
    };

    tracked.pull(&downloader, args.concurrency).await?;
    pb_update_task.await?;

    pb.finish_and_clear();

    let features = HashMap::new();
    let command = GameCommand::from_version_info(
        &file_hierarchy,
        &repository.version_info(),
        &features,
        &args.username,
    );
    let command = command.build("java");
    println!("Game command: {:?}", command);

    Command::from(command).spawn()?.wait().await?;
    Ok(())
}
