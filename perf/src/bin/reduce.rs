

use std::collections::{HashMap, hash_map::RandomState};
use std::time::Duration;
use std::iter::FromIterator;


use structopt::StructOpt;
use clap::arg_enum;

use simplelog::{LevelFilter, SimpleLogger, TermLogger, TerminalMode};
use log::{info};

use itertools::Itertools;

use iot_perf::{Results, DriverMode};

arg_enum! {
    #[derive(Clone, PartialEq, Debug)]
    pub enum Mode {
        None,
    }
}

#[derive(PartialEq, Clone, Debug, StructOpt)]
pub struct Args {
    
    #[structopt(long, default_value = "results/results.json")]
    /// Test results file for processing
    pub results: String,

    #[structopt(long, default_value = "outputs")]
    /// Output directory for re-structured data
    pub output_dir: String,

    #[structopt(long, default_value = "none")]
    /// Restructuring mode
    pub mode: Mode,

    #[structopt(long = "log-level", default_value = "info")]
    /// Configure app logging levels (warn, info, debug, trace)
    pub log_level: LevelFilter,
}

fn main() -> Result<(), anyhow::Error>{
    // Load options
    let opts = Args::from_args();

    // Initialise logging
    let log_config = simplelog::ConfigBuilder::new()
        .build();
    if let Err(_e) = TermLogger::init(opts.log_level, log_config.clone(), TerminalMode::Mixed) {
        SimpleLogger::init(opts.log_level, log_config)?;
    }

    // Load test result data
    info!("Reading test data from: {}", opts.results);
    let results_data = std::fs::read_to_string(&opts.results)?;
    let results: Vec<Results> = serde_json::from_str(&results_data)?;

    // Parse out unique keys
    // Generate per-publisher keys
    let mut publishers: Vec<_> = results.iter().map(|v| v.test.num_publishers).unique().collect();
    publishers.sort();

    let periods: Vec<_> = results.iter().map(|v| v.test.publish_period).unique().collect();
    publishers.sort();
    


    // Perform requested re-structuring

    // Re-structure by mode and period
    let results_by_mode = flatten_mode(&results);
    
    let results_by_mode_publisher = results_by_mode.iter().map(|(k, v)| (k, flatten_publishers(&v)) );

    //let m2 = m1.iter().map(|(k, v)| (k, flatten_period(v.clone())) );

    let modes = HashMap::<_, _, RandomState>::from_iter(results_by_mode_publisher);

    //let mut flattened = vec![];

    for (m, m_results) in &modes {

        info!("Preparing results for mode: {:?}", m);

        let mut filename = format!("{}/{}-cpu.csv", opts.output_dir, m);
        filename.make_ascii_lowercase();

        info!("Writing {}", filename);

        write_stats_by_period(&filename, &publishers, &periods, &m_results, |f| f.cpu_percent.mean )?;

        
        let mut filename = format!("{}/{}-lat.csv", opts.output_dir, m);
        filename.make_ascii_lowercase();

        info!("Writing {}", filename);

        write_stats_by_period(&filename, &publishers, &periods, &m_results, |f| f.latency.mean / 1e3 )?;


        let mut filename = format!("{}/{}-loss.csv", opts.output_dir, m);
        filename.make_ascii_lowercase();

        info!("Writing {}", filename);

        write_stats_by_period(&filename, &publishers, &periods, &m_results, |f| f.packet_loss * 100.0 )?;

        let mut filename = format!("{}/{}-throughput.csv", opts.output_dir, m);
        filename.make_ascii_lowercase();

        info!("Writing {}", filename);

        write_stats_by_period(&filename, &publishers, &periods, &m_results, |f| f.throughput )?;
    }


    Ok(())
}


// Flatten a list of results by mode
fn flatten_mode(results: &[Results]) -> HashMap<DriverMode, Vec<Results>> {
    let mut m = HashMap::<DriverMode, Vec<Results>>::new();

    for r in results {
        m.entry(r.mode.clone())
            .and_modify(|v| v.push(r.clone()) )
            .or_insert(vec![r.clone()]);
    }

    m
}

#[allow(dead_code)]
fn flatten_period(results: &[Results]) -> HashMap<Duration, Vec<Results>> {
    let mut f = HashMap::<Duration, Vec<Results>>::new();

    for r in results {
        f.entry(r.test.publish_period)
            .and_modify(|v| v.push(r.clone()) )
            .or_insert(vec![r.clone()]);
    }

    f
}

fn flatten_publishers(results: &[Results]) -> HashMap<usize, Vec<Results>> {
    let mut f = HashMap::<usize, Vec<Results>>::new();

    for r in results {
        f.entry(r.test.num_publishers)
            .and_modify(|v| v.push(r.clone()) )
            .or_insert(vec![r.clone()]);
    }

    f
}

fn write_stats_by_period<F>(filename: &str, publishers: &[usize], periods: &[Duration], results: &HashMap<usize, Vec<Results>>, filter: F) -> Result<(), anyhow::Error> 
where
    F: Fn(&Results) -> f64,
{

    // Open writer for file
    let mut w = csv::Writer::from_path(filename)?;

    // Generate header
    let mut header = vec![format!("publishers")];
    let mut p: Vec<_> = periods.iter().map(|v| {
        format!("{}", 1.0e3 / v.as_millis() as f64)
    }).collect();
    header.append(&mut p);


    info!("Header: {:?}", header);
    w.serialize(&header)?;

    // Write data for each row
    for n in publishers {

        // Fetch the matching result row for a given number of publishers
        if let Some(r) = results.get(n) {
            let mut row = vec![Some(*n as f64)];

            for p in periods {
                let v = r.iter()
                    .find(|f| f.test.publish_period == *p)
                    .map(|f| filter(f) );
                row.push(v);
            }

            info!("Row: {:?}", row);

            w.serialize(&row)?;
        }
    }

    w.flush()?;

    Ok(())
}