//
// main.rs -- pcgtools core code
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

extern crate clap;
extern crate log;

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io,
    io::{prelude::*, BufReader, Error, ErrorKind},
    path::Path,
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Pathname of PCC file to input
    pccfile: String,

    /// Base directory where PCC and LST files are found
    #[arg(short, long, default_value = ".")]
    datadir: String,
}

#[derive(Serialize, Deserialize)]
enum PccTag {
    Bool,
    Date,
    LstFile,
    Number,
    Text,
    PccFile,
}

#[derive(Serialize, Deserialize)]
pub struct PccElem {
    _ident: String,
    attribs: Vec<(String, String)>,
}

impl PccElem {
    fn new(ident: &str) -> PccElem {
        PccElem {
            _ident: String::from(ident),
            attribs: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct PccList {
    _ident: String,
    props: HashMap<String, PccElem>,
}

impl PccList {
    fn new(ident: &str) -> PccList {
        PccList {
            _ident: String::from(ident),
            props: HashMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum PccDatum {
    Text(String),
    List(PccList),
}

impl PccDatum {
    pub fn as_mut_list(&mut self) -> Option<&mut PccList> {
        match self {
            PccDatum::List(l) => Some(l),
            _ => None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct PccConfig {
    datadir: String,
}

#[derive(Serialize, Deserialize)]
pub struct Pcc {
    config: PccConfig,
    dict: HashMap<String, PccDatum>,
    pcc_schema: HashMap<String, PccTag>,
    aliases: HashMap<String, String>,
}

fn dir_from_path(full_path: &str) -> Option<String> {
    let path = Path::new(full_path);
    path.parent() // Get the parent directory as Option<&Path>
        .and_then(|p| p.to_str()) // Convert &Path to Option<&str>
        .map(|s| s.to_string()) // Convert &str to String
}

fn new_pcc_schema() -> HashMap<String, PccTag> {
    HashMap::from([
        (String::from("PRECAMPAIGN"), PccTag::Text),
        (String::from("BOOKTYPE"), PccTag::Text),
        (String::from("CAMPAIGN"), PccTag::Text),
        (String::from("COMPANIONLIST"), PccTag::Text),
        (String::from("COPYRIGHT"), PccTag::Text),
        (String::from("COVER"), PccTag::Text),
        (String::from("DESC"), PccTag::Text),
        (String::from("DYNAMIC"), PccTag::Text),
        (String::from("FORWARDREF"), PccTag::Text),
        (String::from("GAMEMODE"), PccTag::Text),
        (String::from("GENRE"), PccTag::Text),
        (String::from("HELP"), PccTag::Text),
        (String::from("HIDETYPE"), PccTag::Text),
        (String::from("INFOTEXT"), PccTag::Bool),
        (String::from("ISOGL"), PccTag::Bool),
        (String::from("ISLICENSED"), PccTag::Bool),
        (String::from("KEY"), PccTag::Text),
        (String::from("LOGO"), PccTag::Text),
        (String::from("PCC"), PccTag::PccFile),
        (String::from("PUBNAMELONG"), PccTag::Text),
        (String::from("PUBNAMESHORT"), PccTag::Text),
        (String::from("PUBNAMEWEB"), PccTag::Text),
        (String::from("RANK"), PccTag::Number),
        (String::from("SETTING"), PccTag::Text),
        (String::from("SHOWINMENU"), PccTag::Text),
        (String::from("SOURCEDATE"), PccTag::Date),
        (String::from("SOURCELONG"), PccTag::Text),
        (String::from("SOURCESHORT"), PccTag::Text),
        (String::from("SOURCEWEB"), PccTag::Text),
        (String::from("STATUS"), PccTag::Text),
        (String::from("TYPE"), PccTag::Text),
        (String::from("URL"), PccTag::Text),
        (String::from("ABILITY"), PccTag::LstFile),
        (String::from("ABILITYCATEGORY"), PccTag::LstFile),
        (String::from("ALIGNMENT"), PccTag::LstFile),
        (String::from("ARMORPROF"), PccTag::LstFile),
        (String::from("BIOSET"), PccTag::LstFile),
        (String::from("CLASS"), PccTag::LstFile),
        (String::from("COMPANIONMOD"), PccTag::LstFile),
        (String::from("DATATABLE"), PccTag::LstFile),
        (String::from("DATACONTROL"), PccTag::LstFile), // includes wildcards?
        (String::from("DEITY"), PccTag::LstFile),
        (String::from("DOMAIN"), PccTag::LstFile),
        (String::from("EQUIPMENT"), PccTag::LstFile),
        (String::from("EQUIPMOD"), PccTag::LstFile),
        (String::from("GLOBALMODIFIER"), PccTag::LstFile),
        (String::from("KIT"), PccTag::LstFile),
        (String::from("LANGUAGE"), PccTag::LstFile),
        (String::from("RACE"), PccTag::LstFile),
        (String::from("SAVE"), PccTag::LstFile),
        (String::from("SHIELDPROF"), PccTag::LstFile),
        (String::from("SIZE"), PccTag::LstFile),
        (String::from("SKILL"), PccTag::LstFile),
        (String::from("SPELL"), PccTag::LstFile),
        (String::from("STAT"), PccTag::LstFile),
        (String::from("TEMPLATE"), PccTag::LstFile),
        (String::from("VARIABLE"), PccTag::LstFile),
        (String::from("WEAPONPROF"), PccTag::LstFile),
    ])
}

impl Pcc {
    // create a new Pcc object
    pub fn new(config: &PccConfig) -> Pcc {
        Pcc {
            config: config.clone(),
            dict: HashMap::new(),
            pcc_schema: new_pcc_schema(),
            aliases: HashMap::new(),
        }
    }

    // Read a single LST record
    fn read_lst_line(&mut self, datum: &mut PccDatum, line: &str) -> io::Result<()> {
        // split input by <tab> into tokens
        let mut tokens: Vec<&str> = line.split('\t').collect();

        // the first token is our symbol.  the remainder are attribs.
        let raw_ident = tokens.remove(0);

        // the ".MOD" suffix triggers update of existing elem
        let is_mod = raw_ident.ends_with(".MOD");
        let mut ident;
        if is_mod {
            ident = String::from(&raw_ident[0..(raw_ident.len() - 4)]);
        } else {
            ident = String::from(raw_ident);
        }

        // if ident is an alias, lookup true ident
        match self.aliases.get(&ident) {
            None => {}
            Some(alias) => {
                log::debug!("ALIAS MATCH: {} => {}", ident, alias);
                ident = alias.clone();
            }
        }

        log::debug!("ID={}, is_mod={}", ident, is_mod);

        // gather key=value attribs into a list
        let mut attribs: Vec<(String, String)> = Vec::new();
        for token in &tokens {
            match token.split_once(':') {
                None => {
                    if !token.trim().is_empty() {
                        log::debug!("\t{}", token);
                        attribs.push((token.to_string(), String::from("")));
                    }
                }
                Some((akey, aval)) => {
                    log::debug!("\t{}={}", akey, aval);
                    attribs.push((akey.to_string(), aval.to_string()));
                }
            }
        }

        // pre-processing
        for (key, val) in &attribs {
            match key.as_str() {
                "ABB" => {
                    log::debug!("ALIAS: {}={}", val, ident);
                    self.aliases.insert(val.to_string(), ident.clone());
                }

                "KEY" => {
                    log::debug!("KEY: {}={}", val, ident);
                    ident = val.to_string();
                }

                _ => {}
            }
        }

        // grab ref to list inside datum, for update
        let lst = datum.as_mut_list().unwrap();

        // remove Elem for update, or create new if nonexistent
        let mut obj;
        if lst.props.contains_key(&ident) {
            obj = lst.props.remove(&ident).unwrap();
        } else {
            obj = PccElem::new(&ident);
        }

        // merge new attribs into master attrib list
        for attrib in attribs {
            obj.attribs.push(attrib);
        }

        // push Elem with new attribs back into List
        lst.props.insert(ident.to_string(), obj);

        Ok(())
    }

    // Read LST file into data dictionary
    pub fn read_lst(
        &mut self,
        pcc_tag: &str,
        basedir: &str,
        lstpath: &str,
        lstopts: &str,
    ) -> io::Result<()> {
        let mut fpath = String::new();

        // parse path prefixes
        let prefix = lstpath.chars().next().expect("Empty LST path");
        match prefix {
            // absolute path
            '/' => {
                fpath.push_str(lstpath);
            }

            // base directory is toplevel data dir
            '@' | '*' => {
                let relpath = &lstpath[1..];
                fpath.push_str(&self.config.datadir);
                fpath.push_str(relpath);
            }

            // "local file", in the same directory as PCC file
            _ => {
                fpath.push_str(basedir);
                fpath.push_str("/");
                fpath.push_str(lstpath);
            }
        }

        log::debug!("Pcc.read_lst({}, {}, \"{}\")", pcc_tag, fpath, lstopts);

        let mut datum;

        // Does the List record already exist?  if not, create a new one.
        // Due to "second mutable borrow" issue, we must remove from
        // HashMap, and then insert back into HashMap when we're done.
        if !self.dict.contains_key(pcc_tag) {
            datum = PccDatum::List(PccList::new(pcc_tag));
        } else {
            datum = self.dict.remove(pcc_tag).unwrap();
        }

        // record type check
        match &datum {
            PccDatum::List(_val) => {}
            _ => {
                // todo: technically an error, not a panic
                panic!("key is not a list");
            }
        }

        // open and buffer list file input data
        let file = File::open(fpath)?;
        let rdr = BufReader::new(file);

        // iterate through each text file line
        for line_res in rdr.lines() {
            let line = line_res.expect("BufReader.lst parse failed");

            // comments and empty lines
            let ch = line.chars().next();
            if ch.is_none() || ch == Some('#') {
                continue;
            }

            // parse line
            self.read_lst_line(&mut datum, &line)?;
        }

        // finally, replace updated datum in dictionary
        self.dict.insert(pcc_tag.to_string(), datum);

        Ok(())
    }

    fn read_pcc_line(&mut self, basedir: &str, line: &str) -> io::Result<()> {
        // split on ':'
        let sor = line.split_once(':');
        if sor.is_none() {
            return Err(Error::new(ErrorKind::Other, "PCC invalid line:colon"));
        }

        let mut lhs;
        let rhs;
        (lhs, rhs) = sor.unwrap();
        let _tag_negate;

        if lhs.chars().next() == Some('!') {
            lhs = &lhs[1..];
            _tag_negate = true;
        } else {
            _tag_negate = false;
        }

        // is this tag in the known schema?
        let tagtype_res = self.pcc_schema.get(lhs);
        if tagtype_res.is_none() {
            return Err(Error::new(
                ErrorKind::Other,
                format!("PCC invalid key {}", lhs),
            ));
        }

        let tagtype = tagtype_res.unwrap();
        match tagtype {
            // input included PCC file
            PccTag::PccFile => {
                // relative path indicated by leading '@'
                let (is_rel, fpath);
                if rhs.chars().nth(0) == Some('@') {
                    is_rel = true;
                    fpath = &rhs[1..];
                } else {
                    is_rel = false;
                    fpath = &rhs;
                }

                self.read(fpath, is_rel)?;
            }

            // read LST file
            PccTag::LstFile => match rhs.split_once('|') {
                None => self.read_lst(lhs, &basedir, rhs, String::from("").as_str())?,
                Some((lstpath, lstopts)) => self.read_lst(lhs, &basedir, lstpath, lstopts)?,
            },

            // handle other data types
            PccTag::Bool | PccTag::Date | PccTag::Number | PccTag::Text => {
                // store in global data dictionary
                let tag = self.dict.get_mut(lhs);
                match tag {
                    // new key; store in hashmap
                    None => {
                        self.dict
                            .insert(lhs.to_string(), PccDatum::Text(rhs.to_string()));
                    }

                    // existing key; append to string value
                    Some(datum) => match datum {
                        PccDatum::Text(val) => {
                            val.push_str("\n");
                            val.push_str(rhs);
                        }
                        _ => {}
                    },
                }
            }
        }

        Ok(())
    }

    // recursively read PCC file data into Pcc object
    pub fn read(&mut self, pccpath: &str, is_relative: bool) -> io::Result<()> {
        let mut fpath = String::new();

        if is_relative {
            fpath.push_str(&self.config.datadir);
        }

        fpath.push_str(pccpath);

        if fpath.contains("\\") {
            fpath = fpath.replace("\\", "/");
        }

        let basedir = dir_from_path(&fpath).unwrap();

        log::debug!("Pcc.read({})", fpath);

        let file = File::open(fpath)?;
        let rdr = BufReader::new(file);

        for line_res in rdr.lines() {
            let line = line_res.expect("BufReader parse failed");

            // comments and empty lines
            let ch = line.chars().next();
            if ch.is_none() || ch == Some('#') {
                continue;
            }

            self.read_pcc_line(&basedir, &line)?;
        }

        Ok(())
    }

    // display all data in data dictionary
    pub fn display(&self) {
        println!("{}", serde_json::to_string_pretty(self).unwrap());
    }
}

fn main() {
    env_logger::builder().format_timestamp(None).init();

    // parse command line options
    let args = Args::parse();

    let mut datadir = args.datadir.clone();
    if datadir.chars().last() != Some('/') {
        datadir.push_str("/"); // todo: windows
    }

    // create new Pcc object
    let pcc_cfg = PccConfig { datadir };
    let mut pcc = Pcc::new(&pcc_cfg);

    // recursively read all PCC and LST data, starting at toplevel file
    pcc.read(&args.pccfile, true).expect("PCC.read I/O error");

    // debug: display data dictionary
    pcc.display();
}
