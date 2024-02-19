 use crate::utils::tests::TestFile;

#[test]
fn test_get_conflict_free_file_path() {
    use super::utils::get_conflict_free_file_path;

    {
        let test_file = TestFile::new("/home/mehdi/Projects/flow-rs/10MB-TESTFILE.ORG.pdf");
        let test = get_conflict_free_file_path(&test_file.file_path);
        assert_eq!(
            test,
            "/home/mehdi/Projects/flow-rs/10MB-TESTFILE.ORG (1).pdf"
        );
    }

    {
        let _test_file_1 = TestFile::new("10MB-TESTFILE.ORG.pdf");
        let test_file_2 = TestFile::new("10MB-TESTFILE.ORG (1).pdf");
        let test = get_conflict_free_file_path(&test_file_2.file_path);
        assert_eq!(
            test,
            "10MB-TESTFILE.ORG (2).pdf"
        );
    }

    {
        let test_file = TestFile::new("10MB-TESTFILE.ORG.tar.gz");
        let test = get_conflict_free_file_path(&test_file.file_path);
        assert_eq!(
            test,
            "10MB-TESTFILE.ORG (1).tar.gz"
        );
    }

    {
        let _test_file_1 = TestFile::new("10MB-TESTFILE.ORG.tar.gz");
        let test_file_2 = TestFile::new("10MB-TESTFILE.ORG (1).tar.gz");
        let test = get_conflict_free_file_path(&test_file_2.file_path);
        assert_eq!(
            test,
            "10MB-TESTFILE.ORG (2).tar.gz"
        );
    }
}