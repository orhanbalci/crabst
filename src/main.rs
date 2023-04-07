use chrono::Datelike;
use chrono::NaiveDate;
use chrono::Utc;
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, CellAlignment, Row, Table,
};
use crates_io_api::{AsyncClient, Crate, CratesQueryBuilder, Sort};
use futures::{stream, StreamExt};
use getopts::Options;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use itertools::Itertools;
use rasciigraph::{plot, Config};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tokio::io::{self, AsyncWriteExt};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optopt("c", "", "get single crate download statistics", "CRATE");
    opts.optopt("u", "", "get user download statistics", "USER");
    opts.optopt("o", "", "output format g: graph t: table", "OUTPUT");
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(_) => {
            panic!("failed to read program arguments")
        }
    };

    if matches.opt_present("h") {
        print_usage(&program, opts).await;
        return;
    }

    let today = Utc::now();
    let today_naive = NaiveDate::from_ymd(today.year(), today.month(), today.day());

    if matches.opt_present("c") {
        let crate_name = matches
            .opt_str("c")
            .expect("user did not supplied crate argument");

        let client = AsyncClient::new("stats agent", std::time::Duration::from_millis(2000))
            .expect("can not get client");

        let crate_downloads = client.crate_downloads(&crate_name).await;
        // .expect("can not get crate downloads");
        let api_crate = client
            .get_crate(&crate_name)
            .await
            .expect("can not get detailed information about crate from api");
        match crate_downloads {
            Ok(downloads) => {
                let mut version_downloads = Vec::new();
                for (key, group) in &downloads.version_downloads.iter().group_by(|&vd| vd.date) {
                    let all_version_downloads = group.fold(0, |init, gvd| init + gvd.downloads);
                    version_downloads.push((key, all_version_downloads as f64));
                }
                let dc = version_downloads.iter().map(|vd| vd.1).collect::<Vec<_>>();

                let mut output_type: Option<String> = None;
                if matches.opt_present("o") {
                    output_type = matches.opt_str("o")
                }

                if output_type.unwrap_or_else(|| "t".to_string()) == *"g" {
                    println!(
                        "{}",
                        plot(
                            dc,
                            Config::default()
                                .with_offset(10)
                                .with_height(10)
                                .with_caption(format!(
                                    "{} total downloads {}",
                                    &crate_name, api_crate.crate_data.downloads
                                ))
                        )
                    )
                } else {
                    print_downloads_table(
                        &version_downloads
                            .iter()
                            .map(|t| (format!("{}", t.0), t.1))
                            .collect::<Vec<(String, f64)>>(),
                        api_crate.crate_data.downloads,
                    )
                    .await;
                }
            }
            Err(_) => println!("Failed to get downloads"),
        }
    } else if matches.opt_present("u") {
        let user_name = matches
            .opt_str("u")
            .expect("user did not supply user argument");

        let client = AsyncClient::new("stats agent", std::time::Duration::from_millis(2000))
            .expect("can not get client");

        let user = client
            .user(&user_name)
            .await
            .expect("can not get user information from crates.io");

        let crates = client
            .crates(
                CratesQueryBuilder::new()
                    .page_size(100)
                    .sort(Sort::Alphabetical)
                    .user_id(user.id)
                    .build(),
            )
            .await
            .expect("can not get users crates");

        let crate_daily_downloads: Arc<Mutex<HashMap<String, u64>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("{spinner:.blue} {msg}")
                .unwrap()
                .tick_strings(&[
                    "▹▹▹▹▹",
                    "▸▹▹▹▹",
                    "▹▸▹▹▹",
                    "▹▹▸▹▹",
                    "▹▹▹▸▹",
                    "▹▹▹▹▸",
                    "▪▪▪▪▪",
                ]),
        );
        pb.set_message("Fetching crates infos...");
        let download_futures = stream::iter(crates.crates.clone())
            .map(|crate_info| {
                let client = client.clone();
                let daily_downloads = crate_daily_downloads.clone();
                let inner_pb = pb.clone();
                tokio::spawn(async move {
                    let download_count =
                        get_crate_downloads(&client, &crate_info.name, &today_naive).await;
                    daily_downloads
                        .lock()
                        .await
                        .insert(crate_info.name.clone(), download_count);
                    inner_pb.set_message(format!("Fetching {} info...", crate_info.name));
                    inner_pb.tick();
                })
            })
            .buffer_unordered(50);
        download_futures.collect::<Vec<_>>().await;
        pb.finish_with_message("Finished gathering crate info!");

        let mut output_type: Option<String> = None;
        if matches.opt_present("o") {
            output_type = matches.opt_str("o")
        }

        if output_type.unwrap_or_else(|| "t".to_string()) == *"g" {
            todo!("implement graph output")
        } else {
            print_crates_table(&crates.crates, &crate_daily_downloads.lock().await.clone()).await;
        }
    } else {
        print_usage(&program, opts).await;
    }
}

async fn print_downloads_table(downloads: &[(String, f64)], total: u64) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec!["Date", "Download Count"]);

    let table_rows = downloads.iter().map(|c| {
        Row::from(vec![
            Cell::new(c.0.clone()),
            Cell::new(c.1).set_alignment(CellAlignment::Right),
        ])
    });
    for row in table_rows {
        table.add_row(row);
    }
    table.add_row(vec![
        Cell::new("Total"),
        Cell::new(total).set_alignment(CellAlignment::Right),
    ]);
    let mut stdout = io::stdout();
    let _ = stdout.write_all(format!("{table}").as_bytes()).await;
    let _ = stdout.flush().await;
}

async fn print_crates_table(crates: &[Crate], daily_downloads: &HashMap<String, u64>) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec!["Crate Name", "Download Count", "Daily Downloads"]);
    let table_rows = crates.iter().map(|c| {
        Row::from(vec![
            Cell::new(c.name.clone()),
            Cell::new(c.downloads.to_string()).set_alignment(CellAlignment::Right),
            Cell::new(*daily_downloads.get(&c.name).unwrap_or(&0))
                .set_alignment(CellAlignment::Right),
        ])
    });
    for row in table_rows {
        table.add_row(row);
    }

    table.add_row(Row::from(vec![
        Cell::new("Total"),
        Cell::new(crates.iter().fold(0, |init, c| init + c.downloads))
            .set_alignment(CellAlignment::Right),
        Cell::new(daily_downloads.values().sum::<u64>()).set_alignment(CellAlignment::Right),
    ]));

    let mut stdout = io::stdout();
    let _ = stdout.write_all(format!("{table}").as_bytes()).await;
    let _ = stdout.flush().await;
}

async fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    let mut stdout = io::stdout();
    let _ = stdout
        .write_all(opts.usage(&brief).to_string().as_bytes())
        .await;

    let _ = stdout.flush().await;
}

async fn get_crate_downloads(client: &AsyncClient, crate_name: &str, date: &NaiveDate) -> u64 {
    let crate_downloads = client.crate_downloads(crate_name).await;
    match crate_downloads {
        Ok(downloads) => downloads
            .version_downloads
            .iter()
            .filter(|vd| vd.date == *date)
            .fold(0, |init, crate_download| init + crate_download.downloads),
        _ => 0,
    }
}
