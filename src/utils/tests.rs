use std::fs;

/// Test file that will be removed when its instance is dropped.
/// 
/// Useful for creating temporary files for testing that get cleaned up when a test fails for.
/// 
/// # Example
/// 
/// ```
/// // When this variable gets dropped, the file will be removed
/// let test_file = TestFile::new("10MB-TESTFILE.ORG.pdf");
/// ```
pub struct TestFile {
    pub file_path: String
}

impl TestFile {
    pub fn new(file_path: &str) -> Self {
        fs::File::create(file_path).unwrap();
        TestFile {
            file_path: file_path.to_string()
        }
    }

    fn remove(&self) {
        fs::remove_file(&self.file_path).unwrap();
    }
}

impl Drop for TestFile {
    fn drop(&mut self) {
        self.remove();
    }
}