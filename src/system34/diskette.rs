use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImdTrack {
    pub mode: u8,
    pub cylinder: u8,
    pub head: u8,
    pub sector_ids: Vec<u8>,
}

/// One sector record from an ImageDisk image. Data can be absent for IMD
/// record type 0, and deleted/CRC flags are retained instead of discarded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImdSector {
    pub cylinder: u8,
    pub head: u8,
    pub id: u8,
    pub size: usize,
    pub data: Option<Vec<u8>>,
    pub deleted: bool,
    pub crc_error: bool,
    pub record_type: u8,
}

impl ImdSector {
    pub fn data(&self) -> Option<&[u8]> {
        self.data.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImdImage {
    comment: String,
    tracks: Vec<ImdTrack>,
    sectors: BTreeMap<(u8, u8, u8), ImdSector>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImdError {
    InvalidSignature,
    MissingCommentTerminator,
    Truncated { offset: usize, needed: usize },
    InvalidMode { offset: usize, mode: u8 },
    InvalidSectorSizeCode { offset: usize, code: u8 },
    InvalidSectorRecordType { offset: usize, record_type: u8 },
    DuplicateSector { cylinder: u8, head: u8, sector: u8 },
    Io(String),
}

impl fmt::Display for ImdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "not an ImageDisk (IMD) image"),
            Self::MissingCommentTerminator => {
                write!(f, "IMD comment is missing its 0x1a terminator")
            }
            Self::Truncated { offset, needed } => write!(
                f,
                "truncated IMD image at offset {offset:#x}; need {needed} more bytes"
            ),
            Self::InvalidMode { offset, mode } => {
                write!(f, "invalid IMD track mode {mode} at offset {offset:#x}")
            }
            Self::InvalidSectorSizeCode { offset, code } => {
                write!(
                    f,
                    "invalid IMD sector size code {code} at offset {offset:#x}"
                )
            }
            Self::InvalidSectorRecordType {
                offset,
                record_type,
            } => write!(
                f,
                "invalid IMD sector record type {record_type} at offset {offset:#x}"
            ),
            Self::DuplicateSector {
                cylinder,
                head,
                sector,
            } => write!(f, "duplicate IMD sector C{cylinder:02} H{head} R{sector}"),
            Self::Io(message) => write!(f, "could not read IMD file: {message}"),
        }
    }
}

impl Error for ImdError {}

impl ImdImage {
    pub fn parse(bytes: &[u8]) -> Result<Self, ImdError> {
        if !bytes.starts_with(b"IMD ") {
            return Err(ImdError::InvalidSignature);
        }

        let comment_end = bytes[4..]
            .iter()
            .position(|byte| *byte == 0x1a)
            .map(|position| position + 4)
            .ok_or(ImdError::MissingCommentTerminator)?;
        let comment = String::from_utf8_lossy(&bytes[4..comment_end])
            .trim()
            .to_owned();
        let mut cursor = comment_end + 1;
        let mut tracks = Vec::new();
        let mut sectors = BTreeMap::new();

        while cursor < bytes.len() {
            let track_offset = cursor;
            let header = take(bytes, &mut cursor, 5)?;
            let mode = header[0];
            let track_cylinder = header[1];
            let head_flags = header[2];
            let number_of_sectors = header[3] as usize;
            let size_code = header[4];

            if mode > 5 {
                return Err(ImdError::InvalidMode {
                    offset: track_offset,
                    mode,
                });
            }
            if size_code > 6 {
                return Err(ImdError::InvalidSectorSizeCode {
                    offset: track_offset + 4,
                    code: size_code,
                });
            }

            let sector_size = 128usize << size_code;
            let sector_ids = take(bytes, &mut cursor, number_of_sectors)?.to_vec();
            let cylinder_map = if head_flags & 0x80 != 0 {
                Some(take(bytes, &mut cursor, number_of_sectors)?.to_vec())
            } else {
                None
            };
            let head_map = if head_flags & 0x40 != 0 {
                Some(take(bytes, &mut cursor, number_of_sectors)?.to_vec())
            } else {
                None
            };
            let track_head = head_flags & 0x3f;

            tracks.push(ImdTrack {
                mode,
                cylinder: track_cylinder,
                head: track_head,
                sector_ids: sector_ids.clone(),
            });

            for (index, id) in sector_ids.into_iter().enumerate() {
                let type_offset = cursor;
                let record_type = take(bytes, &mut cursor, 1)?[0];
                let (data, deleted, crc_error) = match record_type {
                    0 => (None, false, false),
                    1 | 3 | 5 | 7 => (
                        Some(take(bytes, &mut cursor, sector_size)?.to_vec()),
                        matches!(record_type, 3 | 7),
                        matches!(record_type, 5 | 7),
                    ),
                    2 | 4 | 6 | 8 => {
                        let fill = take(bytes, &mut cursor, 1)?[0];
                        (
                            Some(vec![fill; sector_size]),
                            matches!(record_type, 4 | 8),
                            matches!(record_type, 6 | 8),
                        )
                    }
                    _ => {
                        return Err(ImdError::InvalidSectorRecordType {
                            offset: type_offset,
                            record_type,
                        })
                    }
                };
                let cylinder = cylinder_map
                    .as_ref()
                    .map_or(track_cylinder, |map| map[index]);
                let head = head_map.as_ref().map_or(track_head, |map| map[index]);
                let key = (cylinder, head, id);
                if sectors.contains_key(&key) {
                    return Err(ImdError::DuplicateSector {
                        cylinder,
                        head,
                        sector: id,
                    });
                }
                sectors.insert(
                    key,
                    ImdSector {
                        cylinder,
                        head,
                        id,
                        size: sector_size,
                        data,
                        deleted,
                        crc_error,
                        record_type,
                    },
                );
            }
        }

        Ok(Self {
            comment,
            tracks,
            sectors,
        })
    }

    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ImdError> {
        let bytes = std::fs::read(path).map_err(|error| ImdError::Io(error.to_string()))?;
        Self::parse(&bytes)
    }

    pub fn comment(&self) -> &str {
        &self.comment
    }

    pub fn tracks(&self) -> &[ImdTrack] {
        &self.tracks
    }

    pub fn sector(&self, cylinder: u8, head: u8, id: u8) -> Option<&ImdSector> {
        self.sectors.get(&(cylinder, head, id))
    }

    pub fn sector_count(&self) -> usize {
        self.sectors.len()
    }

    /// Maps a System/34 sequential sector address. Address 1 begins at
    /// cylinder 1, head 0, sector 1; cylinder 0 is reserved for volume data.
    pub fn sequential_sector(&self, address: u32) -> Option<&ImdSector> {
        if address == 0 {
            return None;
        }

        let mut remaining = address as usize - 1;
        for cylinder in 1..=74 {
            let mut track_sectors: Vec<_> = self
                .sectors
                .range((cylinder, 0, 0)..=(cylinder, 0, u8::MAX))
                .map(|(_, sector)| sector)
                .collect();
            track_sectors.sort_by_key(|sector| sector.id);
            if remaining < track_sectors.len() {
                return track_sectors.get(remaining).copied();
            }
            remaining -= track_sectors.len();
        }
        None
    }
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, count: usize) -> Result<&'a [u8], ImdError> {
    let end = cursor.checked_add(count).ok_or(ImdError::Truncated {
        offset: *cursor,
        needed: count,
    })?;
    if end > bytes.len() {
        return Err(ImdError::Truncated {
            offset: *cursor,
            needed: end - bytes.len(),
        });
    }
    let result = &bytes[*cursor..end];
    *cursor = end;
    Ok(result)
}

/// Logical diskette attachment interface. It resolves physical CHR addresses
/// and returns records synchronously; analog timing and channel cycle-steal
/// behavior are separate layers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DisketteController {
    image: Option<ImdImage>,
    current_cylinder: u8,
    current_head: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisketteError {
    Imd(ImdError),
    NoDisketteMounted,
    TrackNotFound { cylinder: u8, head: u8 },
    SectorNotFound { cylinder: u8, head: u8, sector: u8 },
    SectorUnavailable { cylinder: u8, head: u8, sector: u8 },
    SectorCrcError { cylinder: u8, head: u8, sector: u8 },
    SequentialAddressNotFound { address: u32 },
}

impl fmt::Display for DisketteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Imd(error) => write!(f, "{error}"),
            Self::NoDisketteMounted => write!(f, "no diskette is mounted"),
            Self::TrackNotFound { cylinder, head } => {
                write!(f, "diskette track C{cylinder:02} H{head} was not found")
            }
            Self::SectorNotFound {
                cylinder,
                head,
                sector,
            } => write!(
                f,
                "diskette sector C{cylinder:02} H{head} R{sector} was not found"
            ),
            Self::SectorUnavailable {
                cylinder,
                head,
                sector,
            } => write!(
                f,
                "diskette sector C{cylinder:02} H{head} R{sector} is unavailable"
            ),
            Self::SectorCrcError {
                cylinder,
                head,
                sector,
            } => write!(
                f,
                "diskette sector C{cylinder:02} H{head} R{sector} has a CRC error"
            ),
            Self::SequentialAddressNotFound { address } => {
                write!(f, "sequential diskette sector {address:#x} was not found")
            }
        }
    }
}

impl Error for DisketteError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Imd(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ImdError> for DisketteError {
    fn from(error: ImdError) -> Self {
        Self::Imd(error)
    }
}

impl DisketteController {
    pub fn mount(&mut self, image: ImdImage) {
        self.image = Some(image);
        self.current_cylinder = 0;
        self.current_head = 0;
    }

    pub fn mount_imd(&mut self, bytes: &[u8]) -> Result<(), DisketteError> {
        self.mount(ImdImage::parse(bytes)?);
        Ok(())
    }

    pub fn mount_imd_file(&mut self, path: impl AsRef<Path>) -> Result<(), DisketteError> {
        self.mount(ImdImage::from_file(path)?);
        Ok(())
    }

    pub fn eject(&mut self) {
        self.image = None;
        self.current_cylinder = 0;
        self.current_head = 0;
    }

    pub fn image(&self) -> Option<&ImdImage> {
        self.image.as_ref()
    }

    pub fn position(&self) -> Option<(u8, u8)> {
        self.image
            .as_ref()
            .map(|_| (self.current_cylinder, self.current_head))
    }

    pub fn seek(&mut self, cylinder: u8, head: u8) -> Result<(), DisketteError> {
        let image = self
            .image
            .as_ref()
            .ok_or(DisketteError::NoDisketteMounted)?;
        if !image
            .tracks()
            .iter()
            .any(|track| track.cylinder == cylinder && track.head == head)
        {
            return Err(DisketteError::TrackNotFound { cylinder, head });
        }
        self.current_cylinder = cylinder;
        self.current_head = head;
        Ok(())
    }

    /// Reads a sector by physical cylinder/head/record number. The record's
    /// deleted-data mark is preserved in the returned sector.
    pub fn read_sector(
        &mut self,
        cylinder: u8,
        head: u8,
        sector_id: u8,
    ) -> Result<ImdSector, DisketteError> {
        self.seek(cylinder, head)?;
        let image = self
            .image
            .as_ref()
            .ok_or(DisketteError::NoDisketteMounted)?;
        let sector =
            image
                .sector(cylinder, head, sector_id)
                .ok_or(DisketteError::SectorNotFound {
                    cylinder,
                    head,
                    sector: sector_id,
                })?;
        let data = sector
            .data
            .as_ref()
            .ok_or(DisketteError::SectorUnavailable {
                cylinder,
                head,
                sector: sector_id,
            })?;
        if sector.crc_error {
            return Err(DisketteError::SectorCrcError {
                cylinder,
                head,
                sector: sector_id,
            });
        }
        let mut result = sector.clone();
        result.data = Some(data.clone());
        Ok(result)
    }

    pub fn read_current_sector(&mut self, sector_id: u8) -> Result<ImdSector, DisketteError> {
        let (cylinder, head) = self.position().ok_or(DisketteError::NoDisketteMounted)?;
        self.read_sector(cylinder, head, sector_id)
    }

    pub fn read_sequential_sector(&mut self, address: u32) -> Result<ImdSector, DisketteError> {
        let (cylinder, head, id) = {
            let image = self
                .image
                .as_ref()
                .ok_or(DisketteError::NoDisketteMounted)?;
            let sector = image
                .sequential_sector(address)
                .ok_or(DisketteError::SequentialAddressNotFound { address })?;
            (sector.cylinder, sector.head, sector.id)
        };
        self.read_sector(cylinder, head, id)
    }
}

#[cfg(test)]
mod tests {
    use super::{DisketteController, DisketteError, ImdError, ImdImage};

    fn imd_with_track(cylinder: u8, sector_id: u8, size_code: u8, record: &[u8]) -> Vec<u8> {
        let mut bytes = b"IMD 1.18 test\x1a".to_vec();
        bytes.extend_from_slice(&[0, cylinder, 0, 1, size_code, sector_id]);
        bytes.push(1);
        bytes.extend_from_slice(record);
        bytes.resize(bytes.len() + (128usize << size_code) - record.len(), 0);
        bytes
    }

    #[test]
    fn parses_raw_and_compressed_sector_records() {
        let mut image_bytes = b"IMD test\x1a".to_vec();
        // Track 0, two 128-byte sectors: one raw and one compressed/deleted.
        image_bytes.extend_from_slice(&[0, 0, 0, 2, 0, 1, 2]);
        image_bytes.push(1);
        image_bytes.extend_from_slice(&[0x5a; 128]);
        image_bytes.extend_from_slice(&[4, 0xc4]);

        let image = ImdImage::parse(&image_bytes).unwrap();
        let raw = image.sector(0, 0, 1).unwrap();
        assert_eq!(raw.data().unwrap(), &[0x5a; 128]);
        assert!(!raw.deleted);
        let deleted = image.sector(0, 0, 2).unwrap();
        assert_eq!(deleted.data().unwrap(), &[0xc4; 128]);
        assert!(deleted.deleted);
        assert!(!deleted.crc_error);
    }

    #[test]
    fn rejects_truncated_imd_records() {
        let image = b"IMD test\x1a\x00\x00\x00\x01\x02\x01\x01\x12";
        assert!(matches!(
            ImdImage::parse(image),
            Err(ImdError::Truncated { .. })
        ));
    }

    #[test]
    fn controller_mounts_seeks_and_reads_by_physical_address() {
        let bytes = imd_with_track(1, 3, 2, &[1, 2, 3, 4]);
        let mut controller = DisketteController::default();
        controller.mount_imd(&bytes).unwrap();
        controller.seek(1, 0).unwrap();

        let sector = controller.read_current_sector(3).unwrap();
        assert_eq!(&sector.data().unwrap()[..4], &[1, 2, 3, 4]);
        assert_eq!(sector.data().unwrap().len(), 512);
        assert_eq!(controller.position(), Some((1, 0)));
        assert!(matches!(
            controller.read_current_sector(4),
            Err(DisketteError::SectorNotFound { .. })
        ));
    }

    #[test]
    fn sequential_sector_addressing_starts_on_cylinder_one() {
        let mut bytes = b"IMD test\x1a".to_vec();
        for cylinder in 1..=2 {
            bytes.extend_from_slice(&[0, cylinder, 0, 2, 0, 1, 2]);
            bytes.push(1);
            bytes.extend_from_slice(&[0x10 + cylinder; 128]);
            bytes.push(1);
            bytes.extend_from_slice(&[0x20 + cylinder; 128]);
        }
        let mut controller = DisketteController::default();
        controller.mount_imd(&bytes).unwrap();

        assert_eq!(
            controller
                .read_sequential_sector(1)
                .unwrap()
                .data()
                .unwrap()[0],
            0x11
        );
        assert_eq!(
            controller
                .read_sequential_sector(2)
                .unwrap()
                .data()
                .unwrap()[0],
            0x21
        );
        assert_eq!(
            controller
                .read_sequential_sector(3)
                .unwrap()
                .data()
                .unwrap()[0],
            0x12
        );
    }
}
