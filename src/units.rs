#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteSpan {
  pub start: usize,
  pub end: usize,
}

impl ByteSpan {
  pub fn contains(&self, offset: usize) -> bool {
    let starts_before = self.start <= offset;
    let ends_after = offset < self.end;
    return starts_before && ends_after;
  }
}

pub fn to_offset(position: u32) -> Result<usize, &'static str> {
  return usize::try_from(position).map_err(|_| "source offset does not fit in memory");
}

pub fn to_position(offset: usize) -> Result<u32, &'static str> {
  return u32::try_from(offset).map_err(|_| "source file is larger than 4 GiB");
}

pub fn line_at(source: &[u8], offset: usize) -> usize {
  let clamped = offset.min(source.len());
  let newlines = source[..clamped].iter().filter(|byte| **byte == b'\n').count();
  return newlines + 1;
}
