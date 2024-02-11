extern crate clap;

use clap::Parser;
use std::{
    collections::HashMap,
    fs::File,
    io,
    io::{prelude::*, BufReader, Error, ErrorKind},
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
    datadir: String,
}

pub struct Pcc {
    config: PccConfig,
    dict: HashMap<String, String>,
}

impl Pcc {
    pub fn new(config: &PccConfig) -> Pcc {
        Pcc {
            config: config.clone(),
            dict: HashMap::new(),
        }
    }

    pub fn read(&mut self, relpath: &str) -> io::Result<()> {
        let mut abspath = PathBuf::from(&self.config.datadir);
        abspath.push(relpath);

        println!("PCC-read-lines({})", abspath.as_path().display());

        let file = File::open(abspath)?;
        let rdr = BufReader::new(file);

        for line_res in rdr.lines() {
            let line = line_res.expect("BufReader parse failed");

            let ch = line.chars().next();
            match ch {
                None | Some('#') => {}
                _ => {
                    let sor = line.split_once(':');
                    match sor {
                        None => return Err(Error::new(ErrorKind::Other, "PCC invalid line:colon")),
                        Some((lhs, rhs)) => {
                            self.dict.insert(lhs.to_string(), rhs.to_string());
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn display(&self) {
        for (key, val) in &self.dict {
            println!("{}={}", key, val);
        }
    }
}

fn main() {
    let args = Args::parse();

    let pcc_cfg = PccConfig {
        datadir: args.datadir.clone(),
    };

    let mut pcc = Pcc::new(&pcc_cfg);

    pcc.read(&args.pccfile).expect("Toplevel PCC I/O error");

    pcc.display();

    println!("pcgtools ended.");
}
