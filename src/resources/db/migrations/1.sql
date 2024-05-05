CREATE TABLE IF NOT EXISTS downloads (
    id INTEGER PRIMARY KEY,
    url TEXT NOT NULL,
    status TEXT NOT NULL,
    data_confirmed INTEGER NOT NULL,
    detected_output_file TEXT,
    output_file TEXT,            
    temp_file TEXT,
    resumable INTEGER NOT NULL,
    date_added INTEGER NOT NULL,
    date_completed INTEGER,
    size INTEGER
);
PRAGMA user_version = 1;