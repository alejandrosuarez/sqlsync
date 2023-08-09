use std::{
    collections::BTreeMap,
    io::{self, Write},
    mem::size_of,
};

use crate::{positioned_io::PositionedReader, Serializable};

pub const PAGESIZE: usize = 4096;

pub type PageIdx = u64;
const PAGE_IDX_SIZE: usize = size_of::<PageIdx>();

pub type Page = [u8; PAGESIZE];

#[derive(Default, Debug, Clone)]
pub struct SparsePages {
    pages: BTreeMap<PageIdx, Page>,
}

impl SparsePages {
    pub fn new() -> SparsePages {
        Self {
            pages: BTreeMap::new(),
        }
    }

    pub fn num_pages(&self) -> usize {
        self.pages.len()
    }

    pub fn clear(&mut self) {
        self.pages.clear();
    }

    pub fn write(&mut self, page_idx: PageIdx, page: Page) {
        self.pages.insert(page_idx, page);
    }

    // returns the max page index of this sparse pages object
    pub fn max_page_idx(&self) -> Option<PageIdx> {
        self.pages.keys().max().copied()
    }

    pub fn read(&self, page_idx: PageIdx, page_offset: usize, buf: &mut [u8]) -> usize {
        self.pages
            .get(&page_idx)
            .map(|page| {
                let end = page_offset + buf.len();
                assert!(end <= PAGESIZE, "page offset out of bounds");
                buf.copy_from_slice(&page[page_offset..end]);
                buf.len()
            })
            .unwrap_or(0)
    }
}

/// The serialized form of SparsePages can be read using the SerializedPagesReader object below
impl Serializable for SparsePages {
    fn serialize_into<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        assert!(
            self.pages.len() > 0,
            "cannot serialize empty sparse pages obj"
        );

        // serialize the max page idx
        let max_page_idx = self
            .max_page_idx()
            .expect("cannot serialize empty sparse pages obj");
        writer.write_all(&max_page_idx.to_be_bytes())?;

        // serialize the pages, sorted by page_idx
        for (page_idx, page) in self.pages.iter() {
            writer.write_all(&page_idx.to_be_bytes())?;
            writer.write_all(&page[..])?;
        }

        Ok(())
    }
}

/// Layout is:
///    max_page_idx: u64
///    for each page (sorted by page_idx) [
///      page_idx: u64
///      page: [u8; PAGESIZE]
///    ]
pub struct SerializedPagesReader<R: PositionedReader>(pub R);

impl<R: PositionedReader> SerializedPagesReader<R> {
    pub fn num_pages(&self) -> io::Result<usize> {
        let file_size = self.0.size()?;
        let num_pages = (file_size - PAGE_IDX_SIZE) / (PAGE_IDX_SIZE + PAGESIZE);
        Ok(num_pages)
    }

    pub fn max_page_idx(&self) -> io::Result<PageIdx> {
        let mut buf = [0; PAGE_IDX_SIZE];
        self.0.read_exact_at(0, &mut buf)?;
        Ok(PageIdx::from_be_bytes(buf))
    }

    pub fn read(&self, page_idx: PageIdx, page_offset: usize, buf: &mut [u8]) -> io::Result<usize> {
        assert!(page_offset < PAGESIZE, "page_offset must be < PAGESIZE");
        assert!(
            page_offset + buf.len() <= PAGESIZE,
            "refusing to read more than one page"
        );

        let num_pages = self.num_pages()?;

        let mut left: usize = 0;
        let mut right: usize = num_pages;
        let mut page_idx_buf = [0; PAGE_IDX_SIZE];

        while left < right {
            let mid = left + (right - left) / 2;
            let mid_offset = PAGE_IDX_SIZE + (mid * (PAGE_IDX_SIZE + PAGESIZE));
            self.0.read_exact_at(mid_offset, &mut page_idx_buf)?;

            let mid_idx = PageIdx::from_be_bytes(page_idx_buf);

            if mid_idx == page_idx {
                let read_start = mid_offset + PAGE_IDX_SIZE + page_offset;
                self.0.read_exact_at(read_start, buf)?;
                return Ok(buf.len());
            } else if mid_idx < page_idx {
                left = mid + 1;
            } else {
                right = mid;
            }
        }

        Ok(0)
    }
}