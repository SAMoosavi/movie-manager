use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension, Result, params};

use crate::metadata_extractor::{ImdbMetaData, SeriesMeta, VideoFileData, VideoMetaData};

#[derive(Debug, Clone, serde::Deserialize)]
pub struct FilterValues {
    pub r#type: String,
    #[serde(rename = "minRating")]
    pub min_rating: f64,
    pub country: usize,
    pub genre: usize,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub video_metadata: VideoMetaData,
    pub matched_reasons: Vec<String>, // For debugging/explaining why item matched
}

fn create_conn() -> Result<Connection> {
    Connection::open("movies.db")
}

pub fn create_table() -> Result<()> {
    let conn = create_conn()?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS video_metadata (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            subtitle_path TEXT,
            year INTEGER,
            series_id INTEGER,
            imdb_id TEXT
        );

        CREATE TABLE IF NOT EXISTS series_meta (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            season INTEGER NOT NULL,
            episode INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS video_file_data (
            video_id INTEGER NOT NULL,
            title TEXT NOT NULL,
            path TEXT PRIMARY KEY,
            quality TEXT,
            has_hard_sub INTEGER NOT NULL,
            has_soft_sub INTEGER NOT NULL,
            is_dubbed INTEGER NOT NULL,
            FOREIGN KEY(video_id) REFERENCES video_metadata(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS imdb_metadata (
            imdb_id TEXT PRIMARY KEY,
            title TEXT,
            year TEXT,
            rated TEXT,
            released TEXT,
            runtime TEXT,
            plot TEXT,
            awards TEXT,
            poster TEXT,
            imdb_rating TEXT,
            imdb_votes TEXT,
            box_office TEXT,
            total_seasons TEXT,
            type TEXT
        );

        CREATE TABLE IF NOT EXISTS actors (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE
        );

        CREATE TABLE IF NOT EXISTS imdb_actors (
            imdb_id TEXT,
            actor_id INTEGER,
            PRIMARY KEY (imdb_id, actor_id),
            FOREIGN KEY(imdb_id) REFERENCES imdb_metadata(imdb_id) ON DELETE CASCADE,
            FOREIGN KEY(actor_id) REFERENCES actors(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS directors (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE
        );

        CREATE TABLE IF NOT EXISTS imdb_directors (
            imdb_id TEXT,
            director_id INTEGER,
            PRIMARY KEY (imdb_id, director_id),
            FOREIGN KEY(imdb_id) REFERENCES imdb_metadata(imdb_id) ON DELETE CASCADE,
            FOREIGN KEY(director_id) REFERENCES directors(id) ON DELETE CASCADE
        );
        
        CREATE TABLE IF NOT EXISTS writers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE
        );

        CREATE TABLE IF NOT EXISTS imdb_writers (
            imdb_id TEXT,
            writer_id INTEGER,
            PRIMARY KEY (imdb_id, writer_id),
            FOREIGN KEY(imdb_id) REFERENCES imdb_metadata(imdb_id) ON DELETE CASCADE,
            FOREIGN KEY(writer_id) REFERENCES writers(id) ON DELETE CASCADE
        );
        
        CREATE TABLE IF NOT EXISTS genres (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE
        );

        CREATE TABLE IF NOT EXISTS imdb_genres (
            imdb_id TEXT,
            genre_id INTEGER,
            PRIMARY KEY (imdb_id, genre_id),
            FOREIGN KEY(imdb_id) REFERENCES imdb_metadata(imdb_id) ON DELETE CASCADE,
            FOREIGN KEY(genre_id) REFERENCES genres(id) ON DELETE CASCADE
        );
        
        CREATE TABLE IF NOT EXISTS languages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE
        );

        CREATE TABLE IF NOT EXISTS imdb_languages (
            imdb_id TEXT,
            language_id INTEGER,
            PRIMARY KEY (imdb_id, language_id),
            FOREIGN KEY(imdb_id) REFERENCES imdb_metadata(imdb_id) ON DELETE CASCADE,
            FOREIGN KEY(language_id) REFERENCES languages(id) ON DELETE CASCADE
        );
        
        CREATE TABLE IF NOT EXISTS countries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE
        );

        CREATE TABLE IF NOT EXISTS imdb_countries (
            imdb_id TEXT,
            country_id INTEGER,
            PRIMARY KEY (imdb_id, country_id),
            FOREIGN KEY(imdb_id) REFERENCES imdb_metadata(imdb_id) ON DELETE CASCADE,
            FOREIGN KEY(country_id) REFERENCES countries(id) ON DELETE CASCADE
        );
        ",
    )?;

    Ok(())
}

fn insert_series_meta(conn: &Connection, series: &SeriesMeta) -> Result<u32> {
    conn.execute(
        "INSERT INTO series_meta (season, episode) VALUES (?1, ?2)",
        params![series.season, series.episode],
    )?;
    Ok(conn.last_insert_rowid() as u32)
}

fn insert_imdb_metadata(conn: &Connection, imdb: &ImdbMetaData) -> Result<bool> {
    let changes = conn.execute(
        "INSERT OR IGNORE INTO imdb_metadata (
            imdb_id, title, year, rated, released, runtime, plot, awards, poster,
            imdb_rating, imdb_votes, box_office, total_seasons, type
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            imdb.imdb_id,
            imdb.title,
            imdb.year,
            imdb.rated,
            imdb.released,
            imdb.runtime,
            imdb.plot,
            imdb.awards,
            imdb.poster,
            imdb.imdb_rating,
            imdb.imdb_votes,
            imdb.box_office,
            imdb.total_seasons,
            imdb.r#type
        ],
    )?;
    Ok(changes > 0)
}

fn insert_or_get_id(conn: &Connection, table: &str, field: &str, value: &str) -> Result<u32> {
    let insert_sql = format!("INSERT OR IGNORE INTO {table} ({field}) VALUES (?1)");
    conn.execute(&insert_sql, params![value])?;

    let select_sql = format!("SELECT id FROM {table} WHERE {field} = ?1");
    let mut stmt = conn.prepare(&select_sql)?;
    let id: u32 = stmt.query_row(params![value], |row| row.get(0))?;
    Ok(id)
}

fn insert_join(
    conn: &Connection,
    join_table: &str,
    left_field: &str,
    right_field: &str,
    imdb_id: &str,
    value: &str,
    entity_table: &str,
) -> Result<()> {
    let entity_id = insert_or_get_id(conn, entity_table, "name", value)?;
    let sql = format!("INSERT INTO {join_table} ({left_field}, {right_field}) VALUES (?1, ?2)");
    conn.execute(&sql, params![imdb_id, entity_id])?;
    Ok(())
}

fn insert_imdb_lists(conn: &Connection, imdb: &ImdbMetaData) -> Result<()> {
    for actor in &imdb.actors {
        insert_join(
            conn,
            "imdb_actors",
            "imdb_id",
            "actor_id",
            &imdb.imdb_id,
            actor,
            "actors",
        )?;
    }

    for genre in &imdb.genre {
        insert_join(
            conn,
            "imdb_genres",
            "imdb_id",
            "genre_id",
            &imdb.imdb_id,
            genre,
            "genres",
        )?;
    }

    for director in &imdb.directors {
        insert_join(
            conn,
            "imdb_directors",
            "imdb_id",
            "director_id",
            &imdb.imdb_id,
            director,
            "directors",
        )?;
    }

    for writer in &imdb.writers {
        insert_join(
            conn,
            "imdb_writers",
            "imdb_id",
            "writer_id",
            &imdb.imdb_id,
            writer,
            "writers",
        )?;
    }

    for lang in &imdb.languages {
        insert_join(
            conn,
            "imdb_languages",
            "imdb_id",
            "language_id",
            &imdb.imdb_id,
            lang,
            "languages",
        )?;
    }

    for country in &imdb.country {
        insert_join(
            conn,
            "imdb_countries",
            "imdb_id",
            "country_id",
            &imdb.imdb_id,
            country,
            "countries",
        )?;
    }

    Ok(())
}

fn insert_video_metadata(
    conn: &Connection,
    name: &str,
    subtitle_path: Option<&PathBuf>,
    year: Option<u32>,
    series_id: Option<u32>,
    imdb_id: Option<&str>,
) -> Result<u32> {
    conn.execute(
        "INSERT OR IGNORE INTO video_metadata (name, subtitle_path, year, series_id, imdb_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            name,
            subtitle_path.map(|p| p.to_string_lossy()),
            year,
            series_id,
            imdb_id,
        ],
    )?;
    Ok(conn.last_insert_rowid() as u32)
}

fn insert_video_file_data(conn: &Connection, video_id: u32, file: &VideoFileData) -> Result<()> {
    conn.execute(
        "INSERT INTO video_file_data (
            video_id, title, path, quality,
            has_hard_sub, has_soft_sub, is_dubbed
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            video_id,
            file.title,
            file.path.to_string_lossy(),
            file.quality,
            file.has_hard_sub as i32,
            file.has_soft_sub as i32,
            file.is_dubbed as i32,
        ],
    )?;
    Ok(())
}

pub fn insert(data: &[VideoMetaData]) -> Result<()> {
    let mut conn = create_conn()?;
    let tx = conn.transaction()?;

    for video in data {
        // Insert series if exists
        let series_id = if let Some(series) = &video.series {
            Some(insert_series_meta(&tx, series)?)
        } else {
            None
        };

        // Insert IMDb if exists
        if let Some(imdb) = &video.imdb_metadata {
            if insert_imdb_metadata(&tx, imdb)? {
                insert_imdb_lists(&tx, imdb)?;
            }
        }

        // Insert video metadata
        let video_id = insert_video_metadata(
            &tx,
            &video.name,
            video.subtitle_path.as_ref(),
            video.year,
            series_id,
            video.imdb_metadata.as_ref().map(|x| x.imdb_id.as_str()),
        )?;

        // Insert file data
        for file in &video.files_data {
            insert_video_file_data(&tx, video_id, file)?;
        }
    }

    tx.commit()?;

    Ok(())
}

fn map_row_to_video_file_data(row: &rusqlite::Row) -> Result<VideoFileData> {
    Ok(VideoFileData {
        title: row.get(0)?,
        path: PathBuf::from(row.get::<_, String>(1)?),
        quality: row.get(2)?,
        has_hard_sub: row.get::<_, i32>(3)? != 0,
        has_soft_sub: row.get::<_, i32>(4)? != 0,
        is_dubbed: row.get::<_, i32>(5)? != 0,
    })
}

fn get_video_file_by_path(conn: &Connection, path: PathBuf) -> Result<Option<VideoFileData>> {
    let mut stmt = conn.prepare(
        "SELECT title, path, quality, has_hard_sub, has_soft_sub, is_dubbed
                FROM video_file_data
                WHERE path = ?1",
    )?;

    let path_str = path.to_string_lossy();

    let result = stmt
        .query_row(params![path_str], map_row_to_video_file_data)
        .optional()?;

    Ok(result)
}

fn get_all_video_files(conn: &Connection) -> Result<Vec<VideoFileData>> {
    let mut stmt = conn.prepare("SELECT title, path, quality, has_hard_sub, has_soft_sub, is_dubbed FROM video_file_data").unwrap();
    stmt.query_map([], map_row_to_video_file_data)
        .unwrap()
        .collect()
}

fn remove_row_by_path(conn: &Connection, path: &str) -> Result<usize> {
    conn.execute("DELETE FROM video_file_data WHERE path = ?", [path])
}

fn remove_orphaned_video_metadata(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM video_metadata
         WHERE id NOT IN (
             SELECT DISTINCT video_id FROM video_file_data
         )",
        [],
    )?;
    Ok(())
}

fn get_all_video_metadata(conn: &Connection) -> Result<Vec<VideoMetaData>> {
    let mut stmt = conn
        .prepare("SELECT id, name, subtitle_path, year, series_id, imdb_id FROM video_metadata")?;

    let video_iter = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let subtitle_path: Option<String> = row.get(2)?;
        let year: Option<u32> = row.get(3)?;
        let series_id: Option<i64> = row.get(4)?;
        let imdb_id: Option<String> = row.get(5)?;

        // --- Load files for this video ---
        let files_data = get_video_file_data_by_video_id(conn, id)?;

        // --- Load series if available ---
        let series = match series_id {
            Some(id) => Some(get_series_by_id(conn, id)?),
            None => None,
        };

        // --- Load imdb metadata if available ---
        let imdb_metadata = match imdb_id.clone() {
            Some(ref imdb_id) => Some(get_imdb_metadata(conn, imdb_id)?),
            None => None,
        };

        Ok(VideoMetaData {
            id,
            name,
            subtitle_path: subtitle_path.map(PathBuf::from),
            year,
            files_data,
            series,
            imdb_metadata,
        })
    })?;

    video_iter.collect()
}

fn get_video_file_data_by_video_id(conn: &Connection, video_id: i64) -> Result<Vec<VideoFileData>> {
    let mut stmt = conn.prepare(
        "SELECT title, path, quality, has_hard_sub, has_soft_sub, is_dubbed 
         FROM video_file_data WHERE video_id = ?",
    )?;

    let rows = stmt.query_map(params![video_id], |row| {
        Ok(VideoFileData {
            title: row.get(0)?,
            path: PathBuf::from(row.get::<_, String>(1)?),
            quality: row.get(2)?,
            has_hard_sub: row.get::<_, i32>(3)? != 0,
            has_soft_sub: row.get::<_, i32>(4)? != 0,
            is_dubbed: row.get::<_, i32>(5)? != 0,
        })
    })?;

    rows.collect()
}

fn get_series_by_id(conn: &Connection, id: i64) -> Result<SeriesMeta> {
    conn.query_row(
        "SELECT season, episode FROM series_meta WHERE id = ?",
        params![id],
        |row| {
            Ok(SeriesMeta {
                season: row.get(0)?,
                episode: row.get(1)?,
            })
        },
    )
}

fn get_imdb_metadata(conn: &Connection, imdb_id: &str) -> Result<ImdbMetaData> {
    let base = conn.query_row(
        "SELECT title, year, rated, released, runtime, plot, awards, poster, 
                imdb_rating, imdb_votes, box_office, total_seasons, type 
         FROM imdb_metadata WHERE imdb_id = ?",
        params![imdb_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
            ))
        },
    )?;

    Ok(ImdbMetaData {
        title: base.0,
        year: base.1,
        rated: base.2,
        released: base.3,
        runtime: base.4,
        plot: base.5,
        awards: base.6,
        poster: base.7,
        imdb_rating: base.8,
        imdb_votes: base.9,
        box_office: base.10,
        total_seasons: base.11,
        genre: get_imdb_field(conn, imdb_id, "imdb_genres", "genres")?,
        directors: get_imdb_field(conn, imdb_id, "imdb_directors", "directors")?,
        writers: get_imdb_field(conn, imdb_id, "imdb_writers", "writers")?,
        actors: get_imdb_field(conn, imdb_id, "imdb_actors", "actors")?,
        languages: get_imdb_field(conn, imdb_id, "imdb_languages", "languages")?,
        country: get_imdb_field(conn, imdb_id, "imdb_countries", "countries")?,
        imdb_id: imdb_id.to_string(),
        r#type: base.12,
    })
}

fn get_imdb_field(
    conn: &Connection,
    imdb_id: &str,
    join_table: &str,
    value_table: &str,
) -> Result<Vec<String>> {
    let id_column = match value_table {
        "countries" => "country_id",
        "languages" => "language_id",
        "genres" => "genre_id",
        "writers" => "writer_id",
        "directors" => "director_id",
        "actors" => "actor_id",
        _ => return Err(rusqlite::Error::InvalidQuery), // or define your own error
    };

    let query = format!(
        "SELECT t.name FROM {join_table} j 
         JOIN {value_table} t ON j.{id_column} = t.id WHERE j.imdb_id = ?"
    );

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt
        .query_map(params![imdb_id], |row| row.get::<_, String>(0))?
        .filter_map(Result::ok)
        .collect();

    Ok(rows)
}

fn get_countries(conn: &Connection) -> Result<Vec<(usize, String)>> {
    let mut stmt = conn.prepare("SELECT id, name FROM countries")?;

    let countries = stmt
        .query_map([], |row| {
            Ok((row.get::<_, usize>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(Result::ok)
        .collect();

    Ok(countries)
}

fn get_genres(conn: &Connection) -> Result<Vec<(usize, String)>> {
    let mut stmt = conn.prepare("SELECT id, name FROM genres")?;

    let genres = stmt
        .query_map([], |row| {
            Ok((row.get::<_, usize>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(Result::ok)
        .collect();

    Ok(genres)
}

fn search_videos(conn: &Connection, filters: &FilterValues) -> Result<Vec<VideoMetaData>> {
    let mut where_conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // Build the query with proper joins
    let mut query = r#"
        SELECT DISTINCT 
            vm.id,
            vm.name,
            vm.subtitle_path,
            vm.year,
            vm.series_id,
            vm.imdb_id
        FROM video_metadata vm
        LEFT JOIN imdb_metadata im ON vm.imdb_id = im.imdb_id
    "#
    .to_string();

    // Add conditional joins and conditions
    if filters.r#type != "all" {
        where_conditions.push("im.type = ?".to_string());
        params.push(Box::new(&filters.r#type));
    }

    if filters.min_rating > 0.0 {
        where_conditions.push("CAST(im.imdb_rating AS REAL) >= ?".to_string());
        params.push(Box::new(filters.min_rating));
    }

    if filters.country > 0 {
        query.push_str(" LEFT JOIN imdb_countries ic ON im.imdb_id = ic.imdb_id\n");
        where_conditions.push("ic.country_id = ?".to_string());
        params.push(Box::new(filters.country as i64));
    }

    if filters.genre > 0 {
        query.push_str(" LEFT JOIN imdb_genres ig ON im.imdb_id = ig.imdb_id\n");
        where_conditions.push("ig.genre_id = ?".to_string());
        params.push(Box::new(filters.genre as i64));
    }

    if !filters.name.is_empty() {
        let search_pattern = format!("%{}%", filters.name);
        where_conditions.push("(vm.name LIKE ? OR im.title LIKE ?)".to_string());
        params.push(Box::new(search_pattern.clone()));
        params.push(Box::new(search_pattern));
    }

    // Add WHERE clause if we have conditions
    if !where_conditions.is_empty() {
        query.push_str(&format!(" WHERE {}\n", where_conditions.join(" AND ")));
    }

    query.push_str(" ORDER BY vm.name");

    let mut stmt = conn.prepare(&query)?;
    let results: Result<Vec<Option<VideoMetaData>>, _> = stmt
        .query_map(
            rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
            |row| {
                let video_id: i64 = row.get(0)?;
                get_video_by_id(conn, video_id)
            },
        )?
        .collect();

    // Flatten the Vec<Option<VideoMetaData>> to Vec<VideoMetaData>
    let videos: Vec<VideoMetaData> = results?.into_iter().flatten().collect();

    Ok(videos)
}

fn get_video_by_id(conn: &Connection, video_id: i64) -> Result<Option<VideoMetaData>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, subtitle_path, year, series_id, imdb_id FROM video_metadata WHERE id = ?",
    )?;

    let mut rows = stmt.query_map(params![video_id], |row| {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let subtitle_path: Option<String> = row.get(2)?;
        let year: Option<u32> = row.get(3)?;
        let series_id: Option<i64> = row.get(4)?;
        let imdb_id: Option<String> = row.get(5)?;

        // Load files for this video
        let files_data = get_video_file_data_by_video_id(conn, video_id)?;

        // Load series if available
        let series = match series_id {
            Some(id) => Some(get_series_by_id(conn, id)?),
            None => None,
        };

        // Load imdb metadata if available
        let imdb_metadata = match imdb_id {
            Some(ref imdb_id) => Some(get_imdb_metadata(conn, imdb_id)?),
            None => None,
        };

        Ok(Some(VideoMetaData {
            id,
            name,
            subtitle_path: subtitle_path.map(PathBuf::from),
            year,
            files_data,
            series,
            imdb_metadata,
        }))
    })?;

    if let Some(row) = rows.next() {
        row
    } else {
        Ok(None)
    }
}

pub fn remove_orphaned_video_metadata_from_db() -> Result<()> {
    let conn = create_conn()?;
    remove_orphaned_video_metadata(&conn)
}

pub fn get_all_video_metadata_from_db() -> Result<Vec<VideoMetaData>> {
    let mut conn = create_conn()?;
    let tx = conn.transaction()?;
    let re = get_all_video_metadata(&tx)?;
    tx.commit()?;
    Ok(re)
}

pub fn get_genres_from_db() -> Result<Vec<(usize, String)>> {
    let conn = create_conn()?;
    get_genres(&conn)
}

pub fn get_countries_from_db() -> Result<Vec<(usize, String)>> {
    let conn = create_conn()?;
    get_countries(&conn)
}

pub fn remove_rows_by_paths(paths: &[PathBuf]) -> Result<()> {
    let mut conn = create_conn()?;
    let tx = conn.transaction()?;

    for path in paths {
        // Convert PathBuf to &str
        let path_str = path
            .to_str()
            .ok_or_else(|| rusqlite::Error::InvalidPath(path.to_path_buf()))?;
        remove_row_by_path(&tx, path_str)?;
    }

    tx.commit()
}

pub fn get_all_video_files_from_db() -> Result<Vec<VideoFileData>> {
    let conn = create_conn().unwrap();
    get_all_video_files(&conn)
}

pub fn get_video_file_by_path_from_db(path: PathBuf) -> Result<Option<VideoFileData>> {
    let conn = create_conn()?;
    get_video_file_by_path(&conn, path)
}

pub fn search_videos_on_db(filters: &FilterValues) -> Result<Vec<VideoMetaData>> {
    let mut conn = create_conn()?;
    let tx = conn.transaction()?;
    let re = search_videos(&tx, filters);
    tx.commit()?;
    re
}

pub fn get_video_by_id_from_db(video_id: i64) -> Result<Option<VideoMetaData>> {
    let mut conn = create_conn()?;
    let tx = conn.transaction()?;
    let re = get_video_by_id(&tx, video_id);
    tx.commit()?;
    re
}
