//! NES Memory Mappers for Cartridges
//!
//! <http://wiki.nesdev.com/w/index.php/Mapper>

use crate::{
    common::{Clock, Kind, NesRegion, Reset},
    ppu::Mirroring,
};
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

pub use m000_nrom::Nrom;
pub use m001_sxrom::{Mmc1Revision, Sxrom};
pub use m002_uxrom::Uxrom;
pub use m003_cnrom::Cnrom;
pub use m004_txrom::{Mmc3Revision, Txrom};
pub use m005_exrom::Exrom;
pub use m007_axrom::Axrom;
pub use m009_pxrom::Pxrom;
pub use m024_m026_vrc6::Vrc6;
pub use m066_gxrom::Gxrom;
pub use m071_bf909x::{Bf909Revision, Bf909x};

pub mod m000_nrom;
pub mod m001_sxrom;
pub mod m002_uxrom;
pub mod m003_cnrom;
pub mod m004_txrom;
pub mod m005_exrom;
pub mod m007_axrom;
pub mod m009_pxrom;
pub mod m024_m026_vrc6;
pub mod m066_gxrom;
pub mod m071_bf909x;
pub mod vrc_irq;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[must_use]
pub enum MapperRevision {
    Mmc1(Mmc1Revision),
    Mmc3(Mmc3Revision),
    Bf909(Bf909Revision),
}

#[enum_dispatch]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
#[must_use]
pub enum Mapper {
    Empty,
    Nrom,
    Sxrom,
    Uxrom,
    Cnrom,
    Txrom,
    Exrom,
    Axrom,
    Pxrom,
    Vrc6,
    Gxrom,
    Bf909x,
}

impl Mapper {
    pub fn none() -> Self {
        Empty.into()
    }
}

impl Default for Mapper {
    fn default() -> Self {
        Self::none()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum MappedRead {
    None,
    Chr(usize),
    PrgRom(usize),
    PrgRam(usize),
    Data(u8),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[must_use]
pub enum MappedWrite {
    None,
    Chr(usize, u8),
    PrgRam(usize, u8),
    PrgRamProtect(bool),
}

#[enum_dispatch(Mapper)]
pub trait MemMap {
    #[inline]
    fn map_read(&mut self, addr: u16) -> MappedRead {
        self.map_peek(addr)
    }
    fn map_peek(&self, _addr: u16) -> MappedRead {
        MappedRead::None
    }
    fn map_write(&mut self, _addr: u16, _val: u8) -> MappedWrite {
        MappedWrite::None
    }
}

#[enum_dispatch(Mapper)]
pub trait Mapped {
    #[inline]
    #[must_use]
    fn irq_pending(&self) -> bool {
        false
    }

    #[inline]
    fn mirroring(&self) -> Option<Mirroring> {
        None
    }

    #[inline]
    #[must_use]
    fn use_ciram(&self, _addr: u16) -> bool {
        self.mirroring() != Some(Mirroring::FourScreen)
    }

    #[inline]
    #[must_use]
    fn nametable_page(&self, _addr: u16) -> Option<u16> {
        None
    }

    #[inline]
    fn ppu_addr(&mut self, _addr: u16) {}

    #[inline]
    fn ppu_read(&mut self, _addr: u16) {}

    #[inline]
    fn ppu_write(&mut self, _addr: u16, _val: u8) {}

    #[inline]
    fn set_region(&mut self, _region: NesRegion) {}
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[must_use]
pub struct Empty;

impl MemMap for Empty {}
impl Mapped for Empty {}
impl Clock for Empty {}
impl Reset for Empty {}
