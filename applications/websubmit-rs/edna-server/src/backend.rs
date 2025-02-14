use crate::args;
use crate::args::Connection;
use edna::EdnaClient;
use mysql::prelude::*;
use mysql::Opts;
pub use mysql::Value;
use mysql::*;
use std::time;

pub struct MySqlBackend {
    pub handle: mysql::Conn,
    pub log: slog::Logger,
    pub edna: EdnaClient,
    pub is_baseline: bool,
    connection: Connection,
    user: String,
    pass: String,
    dbname: String,
}

impl MySqlBackend {
    pub fn new(dbname: &str, log: Option<slog::Logger>, args: &args::Args) -> Result<Self> {
        let log = match log {
            None => slog::Logger::root(slog::Discard, o!()),
            Some(l) => l,
        };

        debug!(log, "Connecting to MySql DB {}...", dbname);

        let (db, edna) = match &args.connection {
            Connection::Port(port) => {
                let url = format!(
                    "mysql://{}:{}@127.0.0.1:{}/{}",
                    args.config.mysql_user, args.config.mysql_pass, port, args.class
                );

                let mut db = mysql::Conn::new(Opts::from_url(&url).unwrap()).unwrap();
                assert!(db.ping());

                let edna = EdnaClient::new(
                    &args.config.mysql_user,
                    &args.config.mysql_pass,
                    &format!("127.0.0.1:{}", port),
                    &args.class,
                    true,
                    false,
                    args.dryrun,
                );

                (db, edna)
            }
            Connection::Socket(socket) => {
                let mut db = mysql::Conn::new(
                    OptsBuilder::new()
                        .socket(Some(socket))
                        .user(Some(&args.config.mysql_user))
                        .pass(Some(&args.config.mysql_pass))
                        .db_name(Some(&args.class)),
                )
                .unwrap();
                assert!(db.ping());

                let edna = EdnaClient::with_socket(
                    &args.config.mysql_user,
                    &args.config.mysql_pass,
                    &socket,
                    &args.class,
                    true,
                    args.dryrun,
                );

                (db, edna)
            }
        };

        Ok(MySqlBackend {
            handle: db,
            log,
            edna,
            is_baseline: args.is_baseline,
            connection: args.connection.clone(),
            user: args.config.mysql_user.clone(),
            pass: args.config.mysql_pass.clone(),
            dbname: args.class.clone(),
        })
    }

    fn reconnect(&mut self) {
        self.handle = match &self.connection {
            Connection::Port(port) => {
                let url = format!(
                    "mysql://{}:{}@127.0.0.1:{}/{}",
                    self.user, self.pass, port, self.dbname,
                );
                mysql::Conn::new(Opts::from_url(&url).unwrap()).unwrap()
            }
            Connection::Socket(socket) => mysql::Conn::new(
                OptsBuilder::new()
                    .socket(Some(socket))
                    .user(Some(&self.user))
                    .pass(Some(&self.pass))
                    .db_name(Some(&self.dbname)),
            )
            .unwrap(),
        };
    }

    pub fn query_iter(&mut self, sql: &str) -> Vec<Vec<Value>> {
        // lily: turn this into a single query
        let start = time::Instant::now();
        loop {
            match self.handle.query_iter(sql) {
                Err(e) => {
                    warn!(
                        self.log,
                        "query \'{}\' failed ({}), reconnecting to database", sql, e
                    );
                }
                Ok(res) => {
                    warn!(
                        self.log,
                        "query {}: {}mus",
                        sql,
                        start.elapsed().as_micros()
                    );
                    let start = time::Instant::now();
                    let mut rows = vec![];
                    for row in res {
                        let start = time::Instant::now();
                        let rowvals = row.unwrap().unwrap();
                        let vals: Vec<Value> = rowvals.iter().map(|v| v.clone().into()).collect();
                        debug!(
                            self.log,
                            "Collecting rowval took {}mus",
                            start.elapsed().as_micros()
                        );
                        let start = time::Instant::now();
                        rows.push(vals);
                        debug!(
                            self.log,
                            "pushing vals took {}mus",
                            start.elapsed().as_micros()
                        );
                    }
                    warn!(
                        self.log,
                        "query {} parsing {} rows: {}mus",
                        sql,
                        rows.len(),
                        start.elapsed().as_micros()
                    );
                    debug!(self.log, "executed query {}, got {} rows", sql, rows.len());
                    return rows;
                }
            }
            self.reconnect();
        }
    }

    pub fn exec_batch<P>(&mut self, stmt: &str, params: Vec<P>)
    where
        P: Into<Params> + Clone,
    {
        while let Err(e) = self.handle.exec_batch(stmt, &params) {
            warn!(
                self.log,
                "failed to perform query {} ({}), reconnecting to database", stmt, e
            );
            self.reconnect();
        }
    }

    pub fn query_drop(&mut self, q: &str) {
        let start = time::Instant::now();
        while let Err(e) = self.handle.query_drop(q) {
            warn!(
                self.log,
                "failed to perform query {} ({}), reconnecting to database", q, e
            );
            self.reconnect();
        }
        warn!(self.log, "query {}: {}mus", q, start.elapsed().as_micros());
    }

    fn do_insert(&mut self, table: &str, vals: Vec<Value>, replace: bool) {
        let start = time::Instant::now();
        let _op = if replace { "REPLACE" } else { "INSERT" };
        let q = format!(
            "INSERT INTO {} VALUES ({})",
            table,
            vals.iter()
                .map(|v| v.as_sql(true))
                .collect::<Vec<String>>()
                .join(",")
        );
        debug!(self.log, "executed insert query {} for row {:?}", q, vals);
        while let Err(e) = self.handle.query_drop(q.clone()) {
            warn!(
                self.log,
                "failed to insert into {}, query {} ({}), reconnecting to database", table, q, e
            );
            self.reconnect();
        }
        warn!(self.log, "query {}: {}mus", q, start.elapsed().as_micros());
    }

    pub fn insert(&mut self, table: &str, vals: Vec<Value>) {
        self.do_insert(table, vals, false);
    }

    pub fn update(&mut self, table: &str, vals: Vec<(&str, String)>) {
        let start = time::Instant::now();
        let q = format!(
            "INSERT INTO {} ({}) VALUES ({}) ON DUPLICATE KEY UPDATE {}",
            table,
            vals.iter()
                .map(|(c, _)| format!("{}", c))
                .collect::<Vec<String>>()
                .join(","),
            vals.iter()
                .map(|(_, v)| format!("{}", v))
                .collect::<Vec<String>>()
                .join(","),
            vals.iter()
                .map(|(c, v)| format!("{} = {}", c, v))
                .collect::<Vec<String>>()
                .join(","),
        );
        debug!(self.log, "executed update query {} for row {:?}", q, vals);
        while let Err(e) = self.handle.query_drop(&q) {
            warn!(
                self.log,
                "failed to insert into {}, query {} ({}), reconnecting to database", table, q, e
            );
            self.reconnect();
        }
        warn!(self.log, "query {}: {}mus", q, start.elapsed().as_micros());
    }
}
