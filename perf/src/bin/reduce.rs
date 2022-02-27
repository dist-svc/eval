

use std::collections::{HashMap, hash_map::RandomState};
use std::time::Duration;
use std::iter::FromIterator;


use structopt::StructOpt;

use simplelog::{LevelFilter, SimpleLogger, TermLogger, TerminalMode};
use log::{info};

use itertools::Itertools;

use iot_perf::{Results, DriverMode};
use strum::VariantNames;
use strum_macros::{Display, EnumString, EnumVariantNames};


#[derive(Clone, PartialEq, Debug, Display, EnumString, EnumVariantNames)]
#[strum(serialize_all = "kebab_case")]
pub enum Mode {
    None,
}


#[derive(PartialEq, Clone, Debug, StructOpt)]
pub struct Args {
    
    #[structopt(long, default_value = "results")]
    /// Test results file for processing
    pub results_dir: String,

    #[structopt(long, default_value = "outputs")]
    /// Output directory for re-structured data
    pub output_dir: String,

    #[structopt(long, default_value = "none", possible_values=Mode::VARIANTS)]
    /// Restructuring mode
    pub mode: Mode,

    #[structopt(long)]
    /// Result filters
    pub ignore: Vec<String>,

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
    info!("Reading test data from: {}", opts.results_dir);
    let mut results = vec![];

    for e in std::fs::read_dir(opts.results_dir)? {
        let p = e?.path();
        if !p.is_file() {
            continue;
        }

        let d = std::fs::read_to_string(&p)?;
        let mut r: Vec<Results> = serde_json::from_str(&d)?;

        results.append(&mut r);
    }

    // Parse out unique keys
    // Generate per-publisher keys
    let mut subscribers: Vec<_> = results.iter().map(|v| v.test.num_subscribers).unique().collect();
    subscribers.sort();

    let mut periods: Vec<_> = results.iter().map(|v| v.test.publish_period).unique().collect();
    periods.sort();

    let mut modes: Vec<_> = results.iter().map(|v| v.mode).unique().collect();
    modes.sort();


    // Perform requested re-structuring

    // Split by mode then period
    let results_by_mode = flatten_mode(&results);
    let results_by_mode_subscriber = results_by_mode.iter().map(|(k, v)| (k, flatten_subscribers(&v)) );

    //let m2 = m1.iter().map(|(k, v)| (k, flatten_period(v.clone())) );

    let result_modes = HashMap::<_, _, RandomState>::from_iter(results_by_mode_subscriber);

    //let mut flattened = vec![];

    for (m, m_results) in &result_modes {

        info!("Preparing results for mode: {:?}", m);

        let mut filename = format!("{}/{}-cpu.csv", opts.output_dir, m);
        filename.make_ascii_lowercase();

        info!("Writing {}", filename);

        write_stats_by_period(&filename, &subscribers, &periods, &m_results, |f| f.cpu_percent.mean )?;

        
        let mut filename = format!("{}/{}-lat.csv", opts.output_dir, m);
        filename.make_ascii_lowercase();

        info!("Writing {}", filename);

        write_stats_by_period(&filename, &subscribers, &periods, &m_results, |f| f.latency.mean / 1e3 )?;


        let mut filename = format!("{}/{}-loss.csv", opts.output_dir, m);
        filename.make_ascii_lowercase();

        info!("Writing {}", filename);

        write_stats_by_period(&filename, &subscribers, &periods, &m_results, |f| f.packet_loss * 100.0 )?;

        let mut filename = format!("{}/{}-throughput.csv", opts.output_dir, m);
        filename.make_ascii_lowercase();

        info!("Writing {}", filename);

        write_stats_by_period(&filename, &subscribers, &periods, &m_results, |f| f.throughput )?;
    }

    // Split by period then mode
    let results_by_period = flatten_period(&results);
    let results_by_period_publisher = results_by_period.iter().map(|(k, v)| (k, flatten_subscribers(&v)) );
    let periods = HashMap::<_, _, RandomState>::from_iter(results_by_period_publisher);

    for (p, p_results) in &periods {
        info!("Preparing results for period: {:?}", p);

        let mut filename = format!("{}/cpu-{:0.0}hz.csv", opts.output_dir, (1000.0 / p.as_millis() as f32) );
        filename.make_ascii_lowercase();

        write_stats_by_mode(&filename, &subscribers, &modes, &p_results, |f| f.cpu_percent.mean )?;

        let mut filename = format!("{}/lat-{:0.0}hz.csv", opts.output_dir, (1000.0 / p.as_millis() as f32) );
        filename.make_ascii_lowercase();

        write_stats_by_mode(&filename, &subscribers, &modes, &p_results, |f| f.latency.mean / 1e3 )?;

        let mut filename = format!("{}/loss-{:0.0}hz.csv", opts.output_dir, (1000.0 / p.as_millis() as f32) );
        filename.make_ascii_lowercase();

        write_stats_by_mode(&filename, &subscribers, &modes, &p_results, |f| f.packet_loss * 100.0 )?;

        let mut filename = format!("{}/throughput-{:0.0}hz.csv", opts.output_dir, (1000.0 / p.as_millis() as f32) );
        filename.make_ascii_lowercase();

        write_stats_by_mode(&filename, &subscribers, &modes, &p_results, |f| f.throughput )?;
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

fn flatten_subscribers(results: &[Results]) -> HashMap<usize, Vec<Results>> {
    let mut f = HashMap::<usize, Vec<Results>>::new();

    for r in results {
        f.entry(r.test.num_subscribers)
            .and_modify(|v| v.push(r.clone()) )
            .or_insert(vec![r.clone()]);
    }

    f
}


fn write_stats_by_period<F>(filename: &str, subscribers: &[usize], periods: &[Duration], results: &HashMap<usize, Vec<Results>>, filter: F) -> Result<(), anyhow::Error> 
where
    F: Fn(&Results) -> f64,
{

    // Open writer for file
    let mut w = csv::Writer::from_path(filename)?;

    // Generate header
    let mut header = vec![format!("subscribers")];
    let mut p: Vec<_> = periods.iter().map(|v| {
        format!("{}", 1.0e3 / v.as_millis() as f64)
    }).collect();
    header.append(&mut p);


    info!("Header: {:?}", header);
    w.serialize(&header)?;

    // Write data for each row
    for n in subscribers {

        // Fetch the matching result row for a given number of subscribers
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

fn write_stats_by_mode<F>(filename: &str, subscribers: &[usize], modes: &[DriverMode], results: &HashMap<usize, Vec<Results>>, filter: F) -> Result<(), anyhow::Error> 
where
    F: Fn(&Results) -> f64,
{

    // Open writer for file
    let mut w = csv::Writer::from_path(filename)?;

    // Generate header
    let mut header = vec![format!("subscribers")];
    let mut p: Vec<_> = modes.iter().map(|v| {
        format!("{}", v)
    }).collect();
    header.append(&mut p);


    info!("Header: {:?}", header);
    w.serialize(&header)?;

    // Write data for each row
    for n in subscribers {

        // Fetch the matching result row for a given number of publishers
        if let Some(r) = results.get(n) {
            let mut row = vec![Some(*n as f64)];

            for m in modes {
                let v = r.iter()
                    .find(|f| f.mode == *m)
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