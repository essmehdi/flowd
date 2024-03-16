 use reqwest::header::{HeaderMap, CONTENT_DISPOSITION, CONTENT_TYPE};

use crate::utils::tests::TestFile;

use super::utils::get_file_info_from_headers;
use super::utils::get_conflict_free_file_path;

#[test]
fn test_get_conflict_free_file_path() {
    {
        let test_file = TestFile::new("10MB-TESTFILE.ORG.pdf");
        let test = get_conflict_free_file_path(&test_file.file_path);
        assert_eq!(
            test,
            "10MB-TESTFILE.ORG (1).pdf"
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

#[test]
fn test_get_file_info_from_headers_attachment() {

    let url = "https://test.com/testfile";
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_DISPOSITION, 
        "attachment; filename=\"testfile.txt\"".parse().unwrap()
    );

    let test = get_file_info_from_headers(url, &headers);

    assert_eq!(
        test.file_name,
        "testfile.txt"
    );
    assert_eq!(
        test.resumable,
        false
    );
    assert_eq!(
        test.content_type,
        None
    );
}

#[test]
fn test_get_file_info_from_headers_no_attachment() {

    let url = "https://test.com/testfile";
    let mut headers = HeaderMap::new();
    headers.insert(
        CONTENT_DISPOSITION, 
        "filename=\"testfile.txt\"".parse().unwrap()
        );
    
    let test = get_file_info_from_headers(url, &headers);

    assert_eq!(
        test.file_name,
        "testfile.txt"
    );
    assert_eq!(
        test.resumable,
        false
    );
    assert_eq!(
        test.content_type,
        None
    );
}

#[test]
fn test_get_file_info_from_headers_no_headers() {

    let url = "https://test.com/testfile";
    let headers = HeaderMap::new();
    let test = get_file_info_from_headers(url, &headers);

    assert_eq!(
        test.file_name,
        "testfile"
    );
    assert_eq!(
        test.resumable,
        false
    );
    assert_eq!(
        test.content_type,
        None
    );
}

#[test]
fn test_get_file_info_from_headers_no_headers_url_special_chars() {

    let url = "https://test.com/t%C3%A9stfile";
    let headers = HeaderMap::new();
    let test = get_file_info_from_headers(url, &headers);

    assert_eq!(
        test.file_name,
        "téstfile"
    );
    assert_eq!(
        test.resumable,
        false
    );
    assert_eq!(
        test.content_type,
        None
    );
}

#[test]
fn test_get_file_info_from_headers_no_path_segment() {
    
        let url = "https://test.com/";
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE, 
            "text/html".parse().unwrap()
        );    
        let test = get_file_info_from_headers(url, &headers);
    
        assert_eq!(
            test.file_name,
            "download.htm"
        );
        assert_eq!(
            test.resumable,
            false
        );
        assert_eq!(
            test.content_type,
            Some("text/html".to_string())
        );
}

#[test]
fn test_get_file_info_from_headers_no_path_segment_no_content_type() {
    
        let url = "https://test.com/";
        let mut headers = HeaderMap::new();
        let test = get_file_info_from_headers(url, &headers);
    
        assert_eq!(
            test.file_name,
            "download"
        );
        assert_eq!(
            test.resumable,
            false
        );
        assert_eq!(
            test.content_type,
            None
        );
}