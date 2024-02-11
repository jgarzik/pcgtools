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

enum TagType {
    Bool,
    Date,
    Number,
    Text,
    ReadPcc,
}

#[derive(Clone)]
pub struct PccConfig {
    datadir: String,
}

pub struct Pcc {
    config: PccConfig,
    dict: HashMap<String, String>,
    pcc_schema: HashMap<String, TagType>,
}

impl Pcc {
    pub fn new(config: &PccConfig) -> Pcc {
        Pcc {
            config: config.clone(),
            dict: HashMap::new(),
            pcc_schema: HashMap::from([
                (String::from("!PRECAMPAIGN"), TagType::Text),
                (String::from("BOOKTYPE"), TagType::Text),
                (String::from("CAMPAIGN"), TagType::Text),
                (String::from("COPYRIGHT"), TagType::Text),
                (String::from("COVER"), TagType::Text),
                (String::from("FORWARDREF"), TagType::Text),
                (String::from("GAMEMODE"), TagType::Text),
                (String::from("GENRE"), TagType::Text),
                (String::from("INFOTEXT"), TagType::Bool),
                (String::from("ISOGL"), TagType::Bool),
                (String::from("ISLICENSED"), TagType::Bool),
                (String::from("KEY"), TagType::Text),
                (String::from("LOGO"), TagType::Text),
                (String::from("PCC"), TagType::ReadPcc),
                (String::from("PUBNAMELONG"), TagType::Text),
                (String::from("PUBNAMESHORT"), TagType::Text),
                (String::from("PUBNAMEWEB"), TagType::Text),
                (String::from("RANK"), TagType::Number),
                (String::from("SETTING"), TagType::Text),
                (String::from("SHOWINMENU"), TagType::Text),
                (String::from("SOURCEDATE"), TagType::Date),
                (String::from("SOURCELONG"), TagType::Text),
                (String::from("SOURCESHORT"), TagType::Text),
                (String::from("SOURCEWEB"), TagType::Text),
                (String::from("STATUS"), TagType::Text),
                (String::from("TYPE"), TagType::Text),
                (String::from("URL"), TagType::Text),
            ]),
        }
    }

    pub fn read(&mut self, relpath: &str) -> io::Result<()> {
        let mut abspath = PathBuf::from(&self.config.datadir);
        abspath.push(relpath);

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
                            if !self.pcc_schema.contains_key(lhs) {
                                return Err(Error::new(ErrorKind::Other, "PCC invalid key"));
                            }
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
