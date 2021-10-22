
use std::{io::Write};

use structopt::StructOpt;
use strum::VariantNames;
use strum_macros::{EnumString, EnumVariantNames};

use serde_json::Value;

#[derive(Clone, Debug, PartialEq, StructOpt)]
struct Options {
    /// Operating mode
    #[structopt(possible_values=Mode::VARIANTS)]
    pub mode: Mode,

    /// Input file name
    #[structopt(default_value=".")]
    pub input: String,

    /// Output file name
    #[structopt(default_value=".")]
    pub output: String,
}

#[derive(Clone, Debug, PartialEq, StructOpt, EnumString, EnumVariantNames)]
#[strum(serialize_all = "kebab-case")]
enum Mode {
    /// Convert CBOR to JSON
    ToJson,
    /// Convert JSON to CBOR
    ToCbor,
}

fn main() -> Result<(), anyhow::Error> {
    let opts = Options::from_args();

    // Load input file
    let i = if opts.input == "." {
        let mut s = String::new();
        while std::io::stdin().read_line(&mut s)? > 0 {}
        s.as_bytes().to_vec()
    } else {
        std::fs::read(opts.input)?
    };

    let o = match &opts.mode {
        Mode::ToJson => {
            // Parse as CBOR
            let d: Value = serde_cbor::from_slice(&i)?;

            // Encode to JSON
            serde_json::to_vec(&d)?
        },
        Mode::ToCbor => {
            // Parse as JSON
            let v: Value = serde_json::from_slice(&i)?;

            // Encode as CBOR
            serde_cbor::to_vec(&v)?
        },
    };

    if opts.output == "." {
        std::io::stdout().write(&o)?;
    } else {
        std::fs::write(opts.output, o)?;
    }

    Ok(())
}
