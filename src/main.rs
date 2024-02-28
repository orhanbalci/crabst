use chrono::Datelike;
use chrono::NaiveDate;
use chrono::Utc;
use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, CellAlignment, Row, Table,
};
use crates_io_api::ReverseDependencies;
use crates_io_api::{AsyncClient, Crate, CratesQueryBuilder, Sort};
use dotago::Dotago;
use futures::SinkExt;
use futures::{stream, StreamExt};
use getopts::Matches;
use getopts::Options;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use itertools::Itertools;
use rasciigraph::{plot, Config};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{self, AsyncWriteExt};
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optopt(
        "c",
        "crate",
        "get single crate download statistics",
        "CRATE",
    );
    opts.optopt(
        "d",
        "dependents",
        "get crate dependents inpormation",
        "CRATE DEPENDENTS",
    );
    opts.optopt("u", "user", "get user download statistics", "USER");
    opts.optopt("o", "output", "output format g: graph t: table", "OUTPUT");
    opts.optopt("l", "last", "show last n days output", "LAST");
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

    if matches.opt_present("c") {
        handle_crate_option(&matches).await;
    } else if matches.opt_present("u") {
        handle_user_option(&matches).await;
    } else if matches.opt_present("d") {
        handle_dependents_option(&matches).await;
    } else {
        print_usage(&program, opts).await;
    }
}

async fn handle_dependents_option(matches: &Matches) {
    let crate_name = matches
        .opt_str("d")
        .expect("user did not supplied crate argument");

    let client = AsyncClient::new("crabst stats agent", std::time::Duration::from_millis(100))
        .expect("can not get client");

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
    pb.set_message(format!("Fetching crate {} dependent infos...", &crate_name));
    pb.enable_steady_tick(Duration::from_millis(500));
    let dependents = client
        .crate_reverse_dependencies(&crate_name)
        .await
        .expect("can not retrieve crate dependents");
    pb.finish_with_message(format!("fetched {} crate dependents", &crate_name));

    print_crate_dependents(&dependents).await;
}

async fn handle_user_option(matches: &Matches) {
    // let today = Utc::now();
    // let today_naive = NaiveDate::from_ymd_opt(today.year(), today.month(), today.day())
    //     .expect("Invalid date value");

    let user_name = matches
        .opt_str("u")
        .expect("user did not supply user argument");

    let client = AsyncClient::new("crabst stats agent", std::time::Duration::from_millis(100))
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

    // let crate_daily_downloads: Arc<Mutex<HashMap<String, u64>>> =
    //     Arc::new(Mutex::new(HashMap::new()));
    let crate_n_day_downloads: Arc<Mutex<HashMap<String, HashMap<NaiveDate, u64>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let last_n_day = if matches.opt_present("l") {
        matches
            .opt_get("l")
            .expect("number of days not defined")
            .expect("user forget to deefine number of days")
    } else {
        1
    };
    let mut days = Vec::new();
    for i in 0..last_n_day {
        days.push(
            i.days()
                .ago()
                .as_date()
                .expect("undefined date")
                .naive_utc()
                .date(),
        )
    }
    days.reverse();

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
            let n_daily_downloads = crate_n_day_downloads.clone();
            let inner_pb = pb.clone();
            let days_clone = days.clone();
            tokio::spawn(async move {
                let download_count =
                    get_crate_downloads_multi(&client, &crate_info.name, &days_clone).await;
                n_daily_downloads
                    .lock()
                    .await
                    .insert(crate_info.name.clone(), download_count);
                inner_pb.set_message(format!("Fetching {} info...", crate_info.name));
                inner_pb.tick();
            })
        })
        .buffer_unordered(3);
    download_futures.collect::<Vec<_>>().await;
    pb.finish_with_message("Finished gathering crate info!");

    let mut output_type: Option<String> = None;
    if matches.opt_present("o") {
        output_type = matches.opt_str("o")
    }

    if output_type.unwrap_or_else(|| "t".to_string()) == *"g" {
        todo!("implement graph output")
    } else {
        print_crates_table(
            &crates.crates,
            &crate_n_day_downloads.lock().await.clone(),
            &days,
        )
        .await;
    }
}

async fn handle_crate_option(matches: &Matches) {
    let crate_name = matches
        .opt_str("c")
        .expect("user did not supplied crate argument");

    let client = AsyncClient::new("stats agent", std::time::Duration::from_millis(100))
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

            if output_type.unwrap_or_else(|| "t".to_string()) == "g" {
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
    let _ = stdout.write_all(table.to_string().as_bytes()).await;
}

async fn print_crates_table(
    crates: &[Crate],
    daily_downloads: &HashMap<String, HashMap<NaiveDate, u64>>,
    days: &Vec<NaiveDate>,
) {
    let mut table = Table::new();
    let mut header_vec = vec!["Crate Name".to_owned(), "Download Count".to_owned()];
    for date in days {
        header_vec.push(date.format("%Y-%m-%d").to_string())
    }

    let mut default_zero_hash = HashMap::new();
    for day in days {
        default_zero_hash.insert(*day, 0);
    }

    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(header_vec);
    let table_rows = crates.iter().map(|c| {
        let mut cell_vec = vec![
            Cell::new(c.name.clone()),
            Cell::new(c.downloads.to_string()).set_alignment(CellAlignment::Right),
        ];
        for day in days {
            cell_vec.push(
                Cell::new(
                    *daily_downloads
                        .get(&c.name)
                        .unwrap_or(&default_zero_hash)
                        .get(day)
                        .unwrap_or(&0),
                )
                .set_alignment(CellAlignment::Right),
            )
        }
        Row::from(cell_vec)
    });
    for row in table_rows {
        table.add_row(row);
    }

    let mut cell_vec = vec![
        Cell::new("Total"),
        Cell::new(crates.iter().fold(0, |init, c| init + c.downloads))
            .set_alignment(CellAlignment::Right),
    ];

    for day in days {
        let total_cell = Cell::new(
            daily_downloads
                .values()
                .map(|download_maps| download_maps.get(day).unwrap_or(&0))
                .sum::<u64>()
                .to_string(),
        )
        .set_alignment(CellAlignment::Right);
        cell_vec.push(total_cell);
    }

    table.add_row(Row::from(cell_vec));

    let mut stdout = io::stdout();
    let _ = stdout.write_all(table.to_string().as_bytes()).await;
}

async fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    let mut stdout = io::stdout();
    let _ = stdout
        .write_all(opts.usage(&brief).to_string().as_bytes())
        .await;
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

async fn get_crate_downloads_multi(
    client: &AsyncClient,
    crate_name: &str,
    dates: &Vec<NaiveDate>,
) -> HashMap<NaiveDate, u64> {
    let crate_downloads = client.crate_downloads(crate_name).await;
    let mut result = HashMap::<NaiveDate, u64>::new();
    dates.iter().for_each(|d| {
        let dcount = match &crate_downloads {
            Ok(downloads) => downloads
                .version_downloads
                .iter()
                .filter(|vd| vd.date == *d)
                .fold(0, |init, crate_download| init + crate_download.downloads),
            _ => 0,
        };
        result.insert(d.clone(), dcount);
    });
    return result;
}

async fn print_crate_dependents(dependents: &ReverseDependencies) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec!["Crate Name", "Download Count"]);
    let table_rows = dependents.dependencies.iter().map(|rd| {
        Row::from(vec![
            Cell::new(rd.crate_version.crate_name.clone()),
            Cell::new(rd.dependency.downloads).set_alignment(CellAlignment::Right),
        ])
    });
    for row in table_rows {
        table.add_row(row);
    }

    let mut stdout = io::stdout();
    let _ = stdout.write_all(table.to_string().as_bytes()).await;
}
