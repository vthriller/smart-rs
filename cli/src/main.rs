#![cfg_attr(feature = "cargo-clippy", allow(print_with_newline))]

#![warn(
	missing_debug_implementations,
	// TODO?..
	//missing_docs,
	//missing_copy_implementations,
	trivial_casts,
	trivial_numeric_casts,
	unsafe_code,
	unstable_features,
	unused_import_braces,
	unused_qualifications,
)]

extern crate hdd;

use hdd::{device, Device};
use hdd::scsi::SCSIDevice;
use hdd::ata::ATADevice;

use hdd::ata::data::id;
use hdd::drivedb;
use hdd::ata::misc::{self, Misc};
use hdd::scsi::ATAError;

#[macro_use]
extern crate clap;
use clap::{
	ArgMatches,
	App,
	AppSettings,
	Values,
};

extern crate serde_json;
extern crate separator;
extern crate number_prefix;
extern crate prettytable;

extern crate log;
extern crate env_logger;
use log::LogLevelFilter;
use env_logger::LogBuilder;

mod info;
mod health;
mod attrs;
mod list;

pub fn when_smart_enabled<F>(status: &id::Ternary, action_name: &str, mut action: F) where F: FnMut() -> () {
	match *status {
		id::Ternary::Unsupported => eprint!("S.M.A.R.T. is not supported, cannot show {}\n", action_name),
		id::Ternary::Disabled => eprint!("S.M.A.R.T. is disabled, cannot show {}\n", action_name),
		id::Ternary::Enabled => action(),
	}
}

#[allow(non_upper_case_globals)]
static drivedb_default: [&'static str; 3] = [
	"/var/lib/smartmontools/drivedb/drivedb.h",
	"/usr/local/share/smartmontools/drivedb.h", // for all FreeBSD folks out there
	"/usr/share/smartmontools/drivedb.h",
];
#[allow(non_upper_case_globals)]
static drivedb_additional_default: [&'static str; 1] = [
	"/etc/smart_drivedb.h",
];

/// Returns concatenated list of entries from main and additional drivedb files, falling back to built-in paths if none were provided.
pub fn open_drivedb(options: Option<Values>) -> Option<Vec<drivedb::Entry>> {
	let options = options
		.map(|vals| vals.collect())
		.unwrap_or_else(|| vec![]);

	let (paths_add, paths_main): (Vec<&str>, Vec<&str>) = options.iter().partition(|path| path.starts_with('+'));

	// trim leading '+'
	let paths_add: Vec<&str> = paths_add.iter().map(|path| &path[1..]).collect();


	let mut show_warn_main = true;
	let mut show_warn_add = true;

	/*
	if some list is empty:
	- apply defaults
	- silence warnings
	*/
	let (paths_main, paths_add) = if paths_main.is_empty() {
		show_warn_main = false;
		let paths_main = drivedb_default.to_vec();

		let paths_add = if paths_add.is_empty() {
			show_warn_add = false;
			drivedb_additional_default.to_vec()
		} else {
			paths_add
		};
		(paths_main, paths_add)
	} else {
		// do not apply defaults to paths_add if paths_main is not the default one
		(paths_main, paths_add)
	};

	let mut entries = Vec::<_>::new();

	// entries from additional files take precedence and, thus, are read first
	for f in paths_add {
		match drivedb::load(f) {
			Ok(fentries) => entries.extend(fentries),
			Err(e) => if show_warn_add {
				eprint!("Cannot open additional drivedb file {}: {}\n", f, e);
			},
		}
	}

	for f in paths_main {
		match drivedb::load(f) {
			Ok(fentries) => {
				entries.extend(fentries);
				break; // we only need one 'main' file, the first valid one
			},
			Err(e) => if show_warn_main {
				eprint!("Cannot open drivedb file {}: {}\n", f, e);
			},
		}
	}

	Some(entries)
}

// cannot use #[cfg(…)] in arg_enum!, hence code duplication

#[cfg(target_os = "linux")]
arg_enum! {
	enum Type { Auto, SAT, SCSI }
}

#[cfg(target_os = "freebsd")]
arg_enum! {
	enum Type { Auto, ATA, SAT, SCSI }
}

#[derive(Debug)]
pub enum DeviceArgument {
	#[cfg(not(target_os = "linux"))]
	ATA(ATADevice<Device>, id::Id),
	SAT(ATADevice<SCSIDevice>, id::Id),
	SCSI(SCSIDevice),
}

type Arg = clap::Arg<'static, 'static>;
pub fn arg_json() -> Arg {
	Arg::with_name("json")
		.long("json")
		.help("Export data in JSON")
}
pub fn arg_drivedb() -> Arg {
	Arg::with_name("drivedb")
			.short("B") // smartctl-like
			.long("drivedb") // smartctl-like
			.takes_value(true)
			.multiple(true)
			.value_name("[+]FILE")
			/*
			TODO show what default values are; now it's not possible, temporary value [0] is short-living and `.help()` only accepts &str, not String
			[0]	format!("…\ndefault:\n{}\n{}",
					drivedb_default.join("\n"),
					drivedb_additional.iter().map(|i| format!("+{}", i)).collect::<Vec<_>>().join("\n"),
				)
			*/
			.help("paths to drivedb files to look for\nuse 'FILE' for main (system-wide) file, '+FILE' for additional entries\nentries are looked up in every additional file in order of their appearance, then in the first valid main file, stopping at the first match\n(this option and its behavior is, to some extent, consistent with '-B' from smartctl)")
}

type F = fn(&Option<&str>, &Option<&DeviceArgument>, &ArgMatches);

fn main() {
	let mut log = LogBuilder::new();

	/*
	XXX this bit of clap.rs lets me down
	we want to allow users to type in types in lower case, but .possible_values() would not allow that unless we pass it modified list of values
	so why do we do it here and not in-place?
	- to_ascii_lowercase() returns `String`s, but .possible_values() only accepts `&str`s, so someone needs to own them. Sigh.
	- the result looks somewhat clunky.

	see also https://github.com/kbknapp/clap-rs/issues/891
	*/
	let type_variants: Vec<_> = Type::variants().iter()
		.map(|s| std::ascii::AsciiExt::to_ascii_lowercase(s.to_owned()))
		.collect();
	// we'll never need original values, so shadow them with the references
	let type_variants: Vec<_> = type_variants.iter()
		.map(|s| &**s)
		.collect();

	let args = App::new("hdd")
		.about("yet another disk querying tool")
		.version(crate_version!())
		.setting(AppSettings::SubcommandRequired)
		.subcommand(health::subcommand())
		.subcommand(list::subcommand())
		.subcommand(info::subcommand())
		.subcommand(attrs::subcommand())
		.arg(Arg::with_name("type")
			.short("t")
			.long("type")
			.takes_value(true)
			.possible_values(type_variants.as_slice())
			.help("device type")
		)
		.arg(Arg::with_name("debug")
			.short("d")
			.long("debug")
			.multiple(true)
			.help("Verbose output: set once to log actions, twice to also show raw data buffers\ncan also be set though env_logger's RUST_LOG env")
		)
		.arg(Arg::with_name("device")
			.help("Device to query")
			//.required(true) // optional for 'list' subcommand, required for anything else
			.index(1)
		)
		.get_matches();

	if let Ok(var) = std::env::var("RUST_LOG") {
		log.parse(&var);
	}
	// -d takes precedence over RUST_LOG which some might export globally for some reasons
	log.filter(Some("hdd"), {
		use self::LogLevelFilter::*;
		match args.occurrences_of("debug") {
			0 => Warn,
			1 => Info,
			_ => Debug,
		}
	});
	log.init().unwrap();

	let path = args.value_of("device");
	let dev = path.map(|p| Device::open(p).unwrap());

	let dtype = args.value_of("type")
		.unwrap_or("auto")
		.parse::<Type>().unwrap();

	let (subcommand, sargs): (F, _) = match args.subcommand() {
		("info", Some(args)) => (info::info, args),
		("health", Some(args)) => (health::health, args),
		("list", Some(args)) => (list::list, args),
		("attrs", Some(args)) => (attrs::attrs, args),
		_ => unreachable!(),
	};

	/*
	Why do we issue ATA IDENTIFY DEVICE here?
	- Device id is what every subcommand uses for one reason or the other, but usually to check whether some feature is supported and enabled.
	- It allows us to distinguish between pure SCSI devices and ATA devices behind SAT by issuing ATA PASS-THROUGH and checking whether this command is supported.
	*/

	let dev = dev.map(|dev| match dtype {
		Type::Auto => {
			match dev.get_type().unwrap() {
				device::Type::SCSI => {
					// check whether devices replies to ATA PASS-THROUGH
					let satdev = ATADevice::new(SCSIDevice::new(dev));
					match satdev.get_device_id() {
						// this is really an ATA device
						Ok(id) =>
							DeviceArgument::SAT(satdev, id),
						// nnnnope, plain SCSI
						Err(misc::Error::SCSI(ATAError::NotSupported)) =>
							DeviceArgument::SCSI(satdev.unwrap()),
						// huh? time to contact Houston
						// TODO? or should we just keep treating devices that return random garbage (Err(ATAError::NoRegisters), weird sense codes &c) as SCSI?
						/*
						e => {
							e.unwrap(); // TODO abort gracefully
							unreachable!() // we already panicked
						},
						*/
						_ => DeviceArgument::SCSI(satdev.unwrap()),
					}
				},
				#[cfg(not(target_os = "linux"))]
				device::Type::ATA => {
					let atadev = ATADevice::new(dev);
					let id = atadev.get_device_id().unwrap();
					DeviceArgument::ATA(atadev, id)
				},
			}
		},
		#[cfg(target_os = "freebsd")]
		Type::ATA => {
			let dev = ATADevice::new(dev);
			let id = dev.get_device_id().unwrap();
			DeviceArgument::ATA(dev, id)
		},
		Type::SAT => {
			let dev = ATADevice::new(SCSIDevice::new(dev));
			let id = dev.get_device_id().unwrap();
			DeviceArgument::SAT(dev, id)
		},
		Type::SCSI => DeviceArgument::SCSI(SCSIDevice::new(dev)),
	});

	subcommand(&path, &dev.as_ref(), sargs)
}
