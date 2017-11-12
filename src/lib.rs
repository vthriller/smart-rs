/*!
This crate allows you to send various commands to storage devices, and to interpret the answers.

## Example

```
use hdd::Device;
use hdd::scsi::SCSIDevice;

let dev = Device::open("/dev/da0").unwrap();
let (sense, data) = dev.scsi_inquiry(vpd, page).unwrap();
```

TODO show how to send hand-crafted commands, or how to use porcelain interfaces.

For more, dive into documentation for the module you're interested in.
*/

#![warn(
	missing_debug_implementations,
	// TODO
	//missing_docs,
	// XXX how to limit this to C-like enums? I'd like to #[derive(Copy)] them
	// see also https://github.com/rust-lang-nursery/rust-clippy/issues/2222
	//missing_copy_implementations,
	trivial_casts,
	trivial_numeric_casts,
	// XXX this crate is all about unsafe code, but we should probably limit that to certain modules
	//unsafe_code,
	unstable_features,
	unused_import_braces,
	unused_qualifications,
)]

#[cfg(feature = "serializable")]
#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate nom;
extern crate regex;
extern crate byteorder;

/// Data transfer direction
#[derive(Debug, Clone, Copy)]
pub enum Direction { None, From, To, Both }

pub mod device;
pub use device::*;

#[cfg(target_os = "freebsd")]
mod cam;

pub mod ata;
pub mod scsi;

pub mod drivedb;