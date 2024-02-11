extern crate clap;

use clap::Parser;
use std::{
    fs::File,
    io,
    io::{prelude::*, BufReader},
    path::Path,
    path::PathBuf,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Pathname of PCC file to input
    pccfile: String,

    /// Directory where PCC and LST files are found
    #[arg(short, long, default_value = ".")]
    datadir: String,
}

#[derive(Clone)]
pub struct PccConfig {
    toplevel: String,
    datadir: String,
}

pub struct Pcc {
    config: PccConfig,
}

fn lines_from_file(filename: impl AsRef<Path>) -> io::Result<Vec<String>> {
    let file = File::open(filename)?;
    let buf = BufReader::new(file);
    Ok(buf
        .lines()
        .map(|l| l.expect("Could not parse line"))
        .collect())
}

impl Pcc {
    pub fn new(config: &PccConfig) -> Pcc {
        Pcc {
            config: config.clone(),
        }
    }

    pub fn read(&self, relpath: &str) -> io::Result<()> {
        let mut abspath = PathBuf::from(&self.config.datadir);
        abspath.push(relpath);

        println!("PCC-read-lines({})", abspath.as_path().display());

        let _lines = lines_from_file(abspath.to_str().expect("BUG"))?;

        Ok(())
    }
}

fn main() {
    let args = Args::parse();

    let pcc_cfg = PccConfig {
        toplevel: args.pccfile.clone(),
        datadir: args.datadir.clone(),
    };

    let pcc = Pcc::new(&pcc_cfg);

    pcc.read(&args.pccfile).expect("Toplevel PCC I/O error");

    println!("Hello, world!");
}
