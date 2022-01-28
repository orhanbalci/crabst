use comfy_table::{
    modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, CellAlignment, Row, Table,
};
use crates_io_api::{Crate, ListOptions, Sort, SyncClient, VersionDownloads};
use getopts::Options;
use itertools::Itertools;
use rasciigraph::{plot, Config};
use std::env;

fn main() {
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
        print_usage(&program, opts);
        return;
    }

    if matches.opt_present("c") {
        let crate_name = matches
            .opt_str("c")
            .expect("user did not supplied crate argument");

        let client = SyncClient::new("stats agent", std::time::Duration::from_millis(2000))
            .expect("can not get client");

        let crate_downloads = client.crate_downloads(&crate_name);
        let api_crate = client
            .get_crate(&crate_name)
            .expect("can not get detailed information about crate from api");
        match crate_downloads {
            Ok(downloads) => {
                if matches.opt_present("o") {
                    let mut version_downloads = Vec::new();
                    for (key, group) in &downloads.version_downloads.iter().group_by(|&vd| vd.date)
                    {
                        let all_version_downloads = group.fold(0, |init, gvd| init + gvd.downloads);
                        version_downloads.push((key, all_version_downloads as f64));
                    }
                    let dc = version_downloads.iter().map(|vd| vd.1).collect::<Vec<_>>();
                    let output_type = matches
                        .opt_str("o")
                        .expect("user did not supplied output argument");
                    if output_type == "g" {
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
                        );
                    }
                }
            }
            Err(_) => println!("Failed to get downloads"),
        }
    } else if matches.opt_present("u") {
        let user_name = matches
            .opt_str("u")
            .expect("user did not supply user argument");

        let client = SyncClient::new("stats agent", std::time::Duration::from_millis(2000))
            .expect("can not get client");

        let user = client
            .users(&user_name)
            .expect("can not get user information from crates.io");

        let crates = client
            .crates(ListOptions {
                page: 1,
                per_page: 100,
                sort: Sort::Alphabetical,
                user_id: Some(user.id.to_string()),
                query: None,
            })
            .expect("can not get users crates");

        if matches.opt_present("o") {
            let output_type = matches
                .opt_str("o")
                .expect("user did not supplied output argument");
            if output_type == "t" {
                print_crates_table(&crates.crates);
            } else {
                todo!("implement graph output")
            }
        }
    } else {
        print_usage(&program, opts);
    }
}

fn print_downloads_table(downloads: &Vec<(String, f64)>, total: u64) {
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
    println!("{table}");
}

fn print_crates_table(crates: &Vec<Crate>) {
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec!["Crate Name", "Download Count"]);
    let table_rows = crates.iter().map(|c| {
        Row::from(vec![
            Cell::new(c.name.clone()),
            Cell::new(c.downloads.to_string()).set_alignment(CellAlignment::Right),
        ])
    });
    for row in table_rows {
        table.add_row(row);
    }

    table.add_row(Row::from(vec![
        Cell::new("Total"),
        Cell::new(crates.iter().fold(0, |init, c| init + c.downloads))
            .set_alignment(CellAlignment::Right),
    ]));

    println!("{table}");
}

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}
