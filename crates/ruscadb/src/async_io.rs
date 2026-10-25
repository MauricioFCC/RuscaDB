#![allow(missing_docs)]
#![allow(dead_code)]
//! I/O asincrono (SPEC-0151): stub sin tokio.

#[derive(Debug, Clone)]
pub struct AsyncReader {
    pub data: Vec<u8>,
}

impl AsyncReader {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
    pub fn read_sync(&self, offset: usize, len: usize) -> Vec<u8> {
        self.data.get(offset..offset + len).unwrap_or(&[]).to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] // @spec AC-0151-01
    fn test_ac_0151_01_async_read() {
        let reader = AsyncReader::new(vec![1, 2, 3, 4, 5]);
        let data = reader.read_sync(1, 3);
        assert_eq!(data, vec![2, 3, 4]);
    }
}
