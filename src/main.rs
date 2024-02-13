extern crate clap;

use clap::Parser;
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

    /// Directory where PCC and LST files are found
    #[arg(short, long, default_value = ".")]
    datadir: String,
}

enum TagType {
    Bool,
    Date,
    List,
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

fn dir_from_path(full_path: &str) -> Option<String> {
    let path = Path::new(full_path);
    path.parent() // Get the parent directory as Option<&Path>
        .and_then(|p| p.to_str()) // Convert &Path to Option<&str>
        .map(|s| s.to_string()) // Convert &str to String
}

fn new_pcc_schema() -> HashMap<String, TagType> {
    HashMap::from([
        (String::from("PRECAMPAIGN"), TagType::Text),
        (String::from("BOOKTYPE"), TagType::Text),
        (String::from("CAMPAIGN"), TagType::Text),
        (String::from("COMPANIONLIST"), TagType::Text),
        (String::from("COPYRIGHT"), TagType::Text),
        (String::from("COVER"), TagType::Text),
        (String::from("DESC"), TagType::Text),
        (String::from("DYNAMIC"), TagType::Text),
        (String::from("FORWARDREF"), TagType::Text),
        (String::from("GAMEMODE"), TagType::Text),
        (String::from("GENRE"), TagType::Text),
        (String::from("HELP"), TagType::Text),
        (String::from("HIDETYPE"), TagType::Text),
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
        (String::from("ABILITY"), TagType::List),
        (String::from("ABILITYCATEGORY"), TagType::List),
        (String::from("ALIGNMENT"), TagType::List),
        (String::from("ARMORPROF"), TagType::List),
        (String::from("BIOSET"), TagType::List),
        (String::from("CLASS"), TagType::List),
        (String::from("COMPANIONMOD"), TagType::List),
        (String::from("DATATABLE"), TagType::List),
        (String::from("DATACONTROL"), TagType::List), // includes wildcards?
        (String::from("DEITY"), TagType::List),
        (String::from("DOMAIN"), TagType::List),
        (String::from("EQUIPMENT"), TagType::List),
        (String::from("EQUIPMOD"), TagType::List),
        (String::from("GLOBALMODIFIER"), TagType::List),
        (String::from("KIT"), TagType::List),
        (String::from("LANGUAGE"), TagType::List),
        (String::from("RACE"), TagType::List),
        (String::from("SAVE"), TagType::List),
        (String::from("SHIELDPROF"), TagType::List),
        (String::from("SIZE"), TagType::List),
        (String::from("SKILL"), TagType::List),
        (String::from("SPELL"), TagType::List),
        (String::from("STAT"), TagType::List),
        (String::from("TEMPLATE"), TagType::List),
        (String::from("VARIABLE"), TagType::List),
        (String::from("WEAPONPROF"), TagType::List),
    ])
}

impl Pcc {
    // create a new Pcc object
    pub fn new(config: &PccConfig) -> Pcc {
        Pcc {
            config: config.clone(),
            dict: HashMap::new(),
            pcc_schema: new_pcc_schema(),
        }
    }

    // Read a single LST record
    fn read_lst_line(&mut self, line: &str) -> io::Result<()> {
        let mut tags: Vec<&str> = line.split('\t').collect();
        let ident = tags.remove(0);
        println!("ID={}, {:?}", ident, tags);
        Ok(())
    }

    // Read LST file into data dictionary
    pub fn read_lst(&mut self, basedir: &str, lstpath: &str, lstopts: &str) -> io::Result<()> {
        let mut fpath = String::new();

        let prefix = lstpath.chars().next().expect("Empty LST path");
        match prefix {
            // todo - don't know how to handle these wildcarded list files yet
            '*' => {
                println!("Pcc.read_lst({}) - SKIPPING", lstpath);
                return Ok(());
            }

            // absolute path
            '/' => {
                fpath.push_str(lstpath);
            }

            // base directory is toplevel data dir
            '@' => {
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

        println!("Pcc.read_lst({}, \"{}\")", fpath, lstopts);

        let file = File::open(fpath)?;
        let rdr = BufReader::new(file);

        for line_res in rdr.lines() {
            let line = line_res.expect("BufReader.lst parse failed");

            // comments and empty lines
            let ch = line.chars().next();
            if ch.is_none() || ch == Some('#') {
                continue;
            }

            self.read_lst_line(&line)?;
        }

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
            TagType::ReadPcc => {
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
            TagType::List => match rhs.split_once('|') {
                None => self.read_lst(&basedir, rhs, String::from("").as_str())?,
                Some((lstpath, lstopts)) => self.read_lst(&basedir, lstpath, lstopts)?,
            },

            // handle other data types
            TagType::Bool | TagType::Date | TagType::Number | TagType::Text => {
                // store in global data dictionary
                let tag = self.dict.get_mut(lhs);
                match tag {
                    // new key; store in hashmap
                    None => {
                        self.dict.insert(lhs.to_string(), rhs.to_string());
                    }

                    // existing key; append to string value
                    Some(val) => {
                        val.push_str("\n");
                        val.push_str(rhs);
                    }
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

        println!("Pcc.read({})", fpath);

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
        for (key, val) in &self.dict {
            println!("{}={}", key, val);
        }
    }
}

fn main() {
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
