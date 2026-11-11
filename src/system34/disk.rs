use std::error::Error;
use std::fmt;
use std::path::Path;

/// Geometry for a raw fixed-disk image. Sector numbers in this API are
/// zero-based; guest I/O commands can translate their native addresses here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskGeometry {
    pub cylinders: u16,
    pub heads: u8,
    pub sectors_per_track: u8,
    pub bytes_per_sector: u16,
}

impl DiskGeometry {
    /// 62EH fixed-disk geometry. Supply the cylinder count for the installed
    /// capacity; the attachment has three heads with 60 256-byte sectors each.
    pub fn system34_62eh(cylinders: u16) -> Result<Self, FixedDiskError> {
        Self::new(cylinders, 3, 60, 256)
    }

    pub fn new(
        cylinders: u16,
        heads: u8,
        sectors_per_track: u8,
        bytes_per_sector: u16,
    ) -> Result<Self, FixedDiskError> {
        let geometry = Self {
            cylinders,
            heads,
            sectors_per_track,
            bytes_per_sector,
        };
        if cylinders == 0 || heads == 0 || sectors_per_track == 0 || bytes_per_sector == 0 {
            return Err(FixedDiskError::InvalidGeometry);
        }
        geometry.byte_len().ok_or(FixedDiskError::InvalidGeometry)?;
        Ok(geometry)
    }

    pub fn sector_count(self) -> usize {
        self.cylinders as usize * self.heads as usize * self.sectors_per_track as usize
    }

    pub fn byte_len(self) -> Option<usize> {
        self.sector_count()
            .checked_mul(self.bytes_per_sector as usize)
    }

    fn sector_offset(self, cylinder: u16, head: u8, sector: u8) -> Option<usize> {
        if cylinder >= self.cylinders || head >= self.heads || sector >= self.sectors_per_track {
            return None;
        }
        let linear_sector = (cylinder as usize * self.heads as usize + head as usize)
            * self.sectors_per_track as usize
            + sector as usize;
        Some(linear_sector * self.bytes_per_sector as usize)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FixedDiskError {
    InvalidGeometry,
    ImageSizeMismatch { expected: usize, actual: usize },
    AddressOutOfRange { cylinder: u16, head: u8, sector: u8 },
    SectorSizeMismatch { expected: usize, actual: usize },
    ByteRangeOutOfRange { start: usize, length: usize },
    NoDiskMounted,
    Io(String),
}

impl fmt::Display for FixedDiskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGeometry => write!(f, "invalid or overflowing fixed-disk geometry"),
            Self::ImageSizeMismatch { expected, actual } => {
                write!(f, "raw disk image must be {expected} bytes, got {actual}")
            }
            Self::AddressOutOfRange {
                cylinder,
                head,
                sector,
            } => write!(
                f,
                "fixed-disk address C{cylinder:03} H{head} R{sector} is outside the image"
            ),
            Self::SectorSizeMismatch { expected, actual } => {
                write!(f, "disk sector must be {expected} bytes, got {actual}")
            }
            Self::ByteRangeOutOfRange { start, length } => write!(
                f,
                "fixed-disk byte range {start:#x}..{:#x} is outside the image",
                start.saturating_add(*length)
            ),
            Self::NoDiskMounted => write!(f, "no fixed-disk image is mounted"),
            Self::Io(message) => write!(f, "could not read fixed-disk image: {message}"),
        }
    }
}

impl Error for FixedDiskError {}

/// Byte-addressable raw disk image with caller-supplied geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixedDiskImage {
    geometry: DiskGeometry,
    data: Vec<u8>,
}

impl FixedDiskImage {
    pub fn blank(geometry: DiskGeometry) -> Result<Self, FixedDiskError> {
        let size = geometry.byte_len().ok_or(FixedDiskError::InvalidGeometry)?;
        Ok(Self {
            geometry,
            data: vec![0; size],
        })
    }

    pub fn from_bytes(geometry: DiskGeometry, data: Vec<u8>) -> Result<Self, FixedDiskError> {
        let expected = geometry.byte_len().ok_or(FixedDiskError::InvalidGeometry)?;
        if data.len() != expected {
            return Err(FixedDiskError::ImageSizeMismatch {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self { geometry, data })
    }

    pub fn from_file(
        path: impl AsRef<Path>,
        geometry: DiskGeometry,
    ) -> Result<Self, FixedDiskError> {
        let data = std::fs::read(path).map_err(|error| FixedDiskError::Io(error.to_string()))?;
        Self::from_bytes(geometry, data)
    }

    pub fn geometry(&self) -> DiskGeometry {
        self.geometry
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn read_sector(
        &self,
        cylinder: u16,
        head: u8,
        sector: u8,
    ) -> Result<&[u8], FixedDiskError> {
        let offset = self.geometry.sector_offset(cylinder, head, sector).ok_or(
            FixedDiskError::AddressOutOfRange {
                cylinder,
                head,
                sector,
            },
        )?;
        let size = self.geometry.bytes_per_sector as usize;
        Ok(&self.data[offset..offset + size])
    }

    /// Reads a contiguous byte range starting at the addressed sector. This is
    /// useful for transferring a raw main-storage image spanning sectors.
    pub fn read_bytes_from(
        &self,
        cylinder: u16,
        head: u8,
        sector: u8,
        length: usize,
    ) -> Result<&[u8], FixedDiskError> {
        let start = self.geometry.sector_offset(cylinder, head, sector).ok_or(
            FixedDiskError::AddressOutOfRange {
                cylinder,
                head,
                sector,
            },
        )?;
        let end = start
            .checked_add(length)
            .filter(|&end| end <= self.data.len())
            .ok_or(FixedDiskError::ByteRangeOutOfRange { start, length })?;
        Ok(&self.data[start..end])
    }

    pub fn write_sector(
        &mut self,
        cylinder: u16,
        head: u8,
        sector: u8,
        bytes: &[u8],
    ) -> Result<(), FixedDiskError> {
        let expected = self.geometry.bytes_per_sector as usize;
        if bytes.len() != expected {
            return Err(FixedDiskError::SectorSizeMismatch {
                expected,
                actual: bytes.len(),
            });
        }
        let offset = self.geometry.sector_offset(cylinder, head, sector).ok_or(
            FixedDiskError::AddressOutOfRange {
                cylinder,
                head,
                sector,
            },
        )?;
        self.data[offset..offset + expected].copy_from_slice(bytes);
        Ok(())
    }
}

/// Fixed-disk attachment and mounted image.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FixedDiskController {
    image: Option<FixedDiskImage>,
}

impl FixedDiskController {
    pub fn mount(&mut self, image: FixedDiskImage) {
        self.image = Some(image);
    }

    pub fn mount_raw_file(
        &mut self,
        path: impl AsRef<Path>,
        geometry: DiskGeometry,
    ) -> Result<(), FixedDiskError> {
        self.mount(FixedDiskImage::from_file(path, geometry)?);
        Ok(())
    }

    pub fn eject(&mut self) {
        self.image = None;
    }

    pub fn image(&self) -> Option<&FixedDiskImage> {
        self.image.as_ref()
    }

    pub fn image_mut(&mut self) -> Option<&mut FixedDiskImage> {
        self.image.as_mut()
    }

    pub fn read_sector(
        &self,
        cylinder: u16,
        head: u8,
        sector: u8,
    ) -> Result<&[u8], FixedDiskError> {
        self.image
            .as_ref()
            .ok_or(FixedDiskError::NoDiskMounted)?
            .read_sector(cylinder, head, sector)
    }

    pub fn read_bytes_from(
        &self,
        cylinder: u16,
        head: u8,
        sector: u8,
        length: usize,
    ) -> Result<&[u8], FixedDiskError> {
        self.image
            .as_ref()
            .ok_or(FixedDiskError::NoDiskMounted)?
            .read_bytes_from(cylinder, head, sector, length)
    }

    pub fn write_sector(
        &mut self,
        cylinder: u16,
        head: u8,
        sector: u8,
        bytes: &[u8],
    ) -> Result<(), FixedDiskError> {
        self.image
            .as_mut()
            .ok_or(FixedDiskError::NoDiskMounted)?
            .write_sector(cylinder, head, sector, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskGeometry, FixedDiskController, FixedDiskError, FixedDiskImage};

    #[test]
    fn raw_fixed_disk_image_reads_and_writes_by_chs() {
        let geometry = DiskGeometry::new(2, 2, 4, 256).unwrap();
        let mut image = FixedDiskImage::blank(geometry).unwrap();
        let sector = vec![0xa5; 256];
        image.write_sector(1, 1, 3, &sector).unwrap();

        assert_eq!(image.read_sector(1, 1, 3).unwrap(), sector);
        assert_eq!(image.bytes().len(), 2 * 2 * 4 * 256);
        assert!(matches!(
            image.read_sector(2, 0, 0),
            Err(FixedDiskError::AddressOutOfRange { .. })
        ));
    }

    #[test]
    fn controller_requires_exact_raw_image_size_and_a_mounted_disk() {
        let geometry = DiskGeometry::new(1, 1, 2, 128).unwrap();
        assert!(matches!(
            FixedDiskImage::from_bytes(geometry, vec![0; 127]),
            Err(FixedDiskError::ImageSizeMismatch { .. })
        ));

        let mut controller = FixedDiskController::default();
        assert!(matches!(
            controller.read_sector(0, 0, 0),
            Err(FixedDiskError::NoDiskMounted)
        ));
        controller.mount(FixedDiskImage::blank(geometry).unwrap());
        controller.write_sector(0, 0, 1, &[0x5a; 128]).unwrap();
        assert_eq!(controller.read_sector(0, 0, 1).unwrap(), &[0x5a; 128]);
    }

    #[test]
    fn system34_62eh_uses_documented_sector_geometry() {
        let geometry = DiskGeometry::system34_62eh(303).unwrap();
        assert_eq!(geometry.heads, 3);
        assert_eq!(geometry.sectors_per_track, 60);
        assert_eq!(geometry.bytes_per_sector, 256);
    }
}
