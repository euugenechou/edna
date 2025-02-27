extern crate clap;
extern crate crypto;
extern crate mysql;
#[macro_use]
extern crate rocket;
extern crate lettre;
extern crate lettre_email;
extern crate rocket_sync_db_pools;
#[macro_use]
extern crate slog;
extern crate log;
extern crate slog_term;
#[macro_use]
extern crate serde_derive;
extern crate base64;

mod admin;
mod apikey;
mod args;
mod backend;
mod config;
mod disguises;
mod email;
mod login;
mod privacy;
mod questions;

use backend::MySqlBackend;
use edna::helpers;
use edna::helpers::Connection;
use mysql::from_value;
use mysql::prelude::*;
use mysql::OptsBuilder;
use mysql::{Opts, Value};
use rocket::fs::FileServer;
use rocket::http::ContentType;
use rocket::http::CookieJar;
use rocket::http::Status;
use rocket::local::blocking::Client;
use rocket::response::Redirect;
use rocket::{Build, Rocket, State};
use rocket_dyn_templates::Template;
use std::cmp::min;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time;
use std::time::Duration;

pub const NITERS: usize = 200;
pub const APIKEY_FILE: &'static str = "apikey.txt";
pub const SHARE_FILE: &'static str = "share.txt";
pub const DID_FILE: &'static str = "dids.txt";

pub fn new_logger() -> slog::Logger {
    use slog::Drain;
    use slog::Logger;
    use slog_term::term_full;
    Logger::root(Mutex::new(term_full()).fuse(), o!())
}

#[get("/")]
fn index(cookies: &CookieJar<'_>, bg: &State<Arc<Mutex<MySqlBackend>>>) -> Redirect {
    if let Some(cookie) = cookies.get("apikey") {
        let apikey: String = cookie.value().parse().ok().unwrap();
        if let Some(cookie) = cookies.get("anonkey") {
            let anonkey: String = cookie.value().parse().ok().unwrap();
            // TODO validate API key
            match apikey::check_api_key(&*bg, &apikey, &anonkey) {
                Ok(_user) => Redirect::to("/leclist"),
                Err(_) => Redirect::to("/login"),
            }
        } else {
            Redirect::to("/login")
        }
    } else {
        Redirect::to("/login")
    }
}

fn rocket(args: &args::Args) -> Rocket<Build> {
    let backend = Arc::new(Mutex::new(
        MySqlBackend::new(&format!("{}", args.class), Some(new_logger()), &args).unwrap(),
    ));

    /*let template_dir = args.config.template_dir.clone();
    let template = Template::custom(move |engines| {
        engines
            .handlebars
            .register_templates_directory(".hbs", std::path::Path::new(&template_dir))
            .expect("failed to set template path!");
    });*/

    rocket::build()
        .attach(Template::fairing())
        //.attach(template)
        .manage(backend)
        .manage(args.config.clone())
        .mount(
            "/css",
            FileServer::from(format!("{}/css", args.config.resource_dir)),
        )
        .mount(
            "/js",
            FileServer::from(format!("{}/js", args.config.resource_dir)),
        )
        .mount("/", routes![index])
        .mount(
            "/questions",
            routes![questions::questions, questions::questions_submit],
        )
        .mount("/apikey/login", routes![apikey::login])
        .mount("/apikey/logout", routes![apikey::logout])
        .mount("/apikey/generate", routes![apikey::generate])
        .mount("/answers", routes![questions::answers])
        .mount("/leclist", routes![questions::leclist])
        .mount("/login", routes![login::login])
        .mount(
            "/admin/lec/add",
            routes![admin::lec_add, admin::lec_add_submit],
        )
        .mount("/admin/users", routes![admin::get_registered_users])
        .mount(
            "/admin/lec",
            routes![admin::lec, admin::addq, admin::editq, admin::editq_submit],
        )
        .mount("/delete", routes![privacy::delete_submit])
        .mount(
            "/admin/anonymize",
            routes![privacy::anonymize, privacy::anonymize_answers],
        )
        .mount(
            "/restore",
            routes![privacy::restore_account, privacy::restore],
        )
        .mount(
            "/anon/auth",
            routes![privacy::edit_as_pseudoprincipal_auth_request],
        )
        .mount("/anon/edit", routes![privacy::edit_as_pseudoprincipal])
}

fn populate_db(
    user2apikey: &mut HashMap<String, String>,
    account_durations: &mut Vec<Duration>,
    args: &args::Args,
    client: &Client,
    db: &mut mysql::Conn,
    log: slog::Logger,
) {
    // create admin
    info!(log, "Creating admin");
    let postdata = serde_urlencoded::to_string(&vec![("email", config::ADMIN.0)]).unwrap();
    let response = client
        .post("/apikey/generate")
        .body(postdata)
        .header(ContentType::Form)
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    // create all users
    for u in 0..args.nusers {
        let email = format!("{}@mail.edu", u);
        let postdata = serde_urlencoded::to_string(&vec![("email", email.clone())]).unwrap();
        let start = time::Instant::now();
        let response = client
            .post("/apikey/generate")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        account_durations.push(start.elapsed());
        assert_eq!(response.status(), Status::Ok);

        // get api key
        let file = File::open(format!("{}.{}", email, APIKEY_FILE)).unwrap();
        let mut buf_reader = BufReader::new(file);
        let mut apikey = String::new();
        buf_reader.read_to_string(&mut apikey).unwrap();
        info!(log, "Got email {} with apikey {}", &email, apikey);
        user2apikey.insert(email.clone(), apikey);
    }

    // initialize for testing
    if args.prime {
        for l in 0..args.nlec {
            db.query_drop(&format!("INSERT INTO lectures VALUES ({}, 'lec{}');", l, l))
                .unwrap();
            for q in 0..args.nqs {
                db.query_drop(&format!(
                    "INSERT INTO questions VALUES ({}, {}, 'lec{}question{}');",
                    l, q, l, q
                ))
                .unwrap();
                for u in 0..args.nusers {
                    // LOGIN
                    let email = format!("{}@mail.edu", u);
                    let apikey = user2apikey.get(&email).unwrap();
                    let postdata =
                        serde_urlencoded::to_string(&vec![("email", &email), ("key", apikey)])
                            .unwrap();
                    let response = client
                        .post("/apikey/login")
                        .body(postdata)
                        .header(ContentType::Form)
                        .dispatch();
                    assert_eq!(response.status(), Status::SeeOther);

                    // insert answers
                    db.query_drop(&format!("INSERT INTO answers VALUES ('{}@mail.edu', {}, {}, 'lec{}q{}answer{}', '1000-01-01 00:00:00');",
                        u, l, q, l, q, u)).unwrap();

                    // logout
                    let response = client.post("/apikey/logout").dispatch();
                    assert_eq!(response.status(), Status::SeeOther);
                }
            }
        }
    }
}

#[rocket::main]
async fn main() {
    env_logger::init();
    let args = args::parse_args();

    if args.prime {
        let schema = std::fs::read_to_string(&args.schema).unwrap();
        match &args.connection {
            Connection::Port(port) => {
                let host = format!("127.0.0.1:{}", port);
                helpers::init_db(
                    false, // in-memory
                    &args.config.mysql_user,
                    &args.config.mysql_pass,
                    &host,
                    &args.class,
                    &schema,
                );
            }
            Connection::Socket(socket) => {
                helpers::init_db_with_socket(
                    false, // in-memory
                    &args.config.mysql_user,
                    &args.config.mysql_pass,
                    socket,
                    &args.class,
                    &schema,
                );
            }
        }
    }

    // create Edna after DB?
    let my_rocket = rocket(&args);

    if args.benchmark {
        if args.is_baseline {
            thread::spawn(move || {
                run_baseline_benchmark(&args, my_rocket);
            })
            .join()
            .expect("Thread panicked")
        } else {
            thread::spawn(move || {
                run_benchmark(&args, my_rocket);
            })
            .join()
            .expect("Thread panicked")
        }
    } else {
        let _ = my_rocket.launch().await.expect("Failed to launch rocket");
    }
}

fn run_baseline_benchmark(args: &args::Args, rocket: Rocket<Build>) {
    let mut account_durations = vec![];
    let mut edit_durations = vec![];
    let mut delete_durations = vec![];
    let mut anon_durations = vec![];
    let mut leclist_durations = vec![];
    let mut answers_durations = vec![];
    let mut questions_durations = vec![];
    let log = new_logger();

    let mut user2apikey = HashMap::new();
    let client = Client::tracked(rocket).expect("valid rocket instance");

    let mut db = match &args.connection {
        Connection::Port(port) => {
            let url = format!(
                "mysql://{}:{}@127.0.0.1:{}/{}",
                args.config.mysql_user, args.config.mysql_pass, port, args.class
            );
            mysql::Conn::new(Opts::from_url(&url).unwrap()).unwrap()
        }
        Connection::Socket(socket) => mysql::Conn::new(
            OptsBuilder::new()
                .socket(Some(socket))
                .user(Some(&args.config.mysql_user))
                .pass(Some(&args.config.mysql_pass))
                .db_name(Some(&args.class)),
        )
        .unwrap(),
    };

    populate_db(
        &mut user2apikey,
        &mut account_durations,
        args,
        &client,
        &mut db,
        log.clone(),
    );

    /************
     * admin read
     *************/
    {
        // login as the admin
        let postdata = serde_urlencoded::to_string(&vec![
            ("email", config::ADMIN.0),
            ("key", config::ADMIN.1),
        ])
        .unwrap();
        let response = client
            .post("/apikey/login")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);

        for _ in 0..NITERS {
            // admin read
            let start = time::Instant::now();
            let response = client.get(format!("/leclist")).dispatch();
            assert_eq!(response.status(), Status::Ok);
            leclist_durations.push(start.elapsed());

            // answers
            let start = time::Instant::now();
            let response = client.get(format!("/answers/{}", 0)).dispatch();
            assert_eq!(response.status(), Status::Ok);
            answers_durations.push(start.elapsed());
        }

        // logout
        let response = client.post("/apikey/logout").dispatch();
        assert_eq!(response.status(), Status::SeeOther);
    }

    /**********************************
     * baseline reads
     ***********************************/
    for u in 0..min(args.nusers, NITERS) {
        let email = format!("{}@mail.edu", u);
        let apikey = user2apikey.get(&email).unwrap();

        // set api key
        let postdata =
            serde_urlencoded::to_string(&vec![("email", &email), ("key", apikey)]).unwrap();
        let response = client
            .post("/apikey/login")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);

        // questions
        let start = time::Instant::now();
        let response = client.get(format!("/questions/{}", 0)).dispatch();
        assert_eq!(response.status(), Status::Ok);
        questions_durations.push(start.elapsed());
    }

    /**********************************
     * baseline edits + delete
     ***********************************/
    for u in 0..min(args.nusers, NITERS) {
        let email = format!("{}@mail.edu", u);
        let apikey = user2apikey.get(&email).unwrap();

        // set api key
        let postdata =
            serde_urlencoded::to_string(&vec![("email", &email), ("key", apikey)]).unwrap();
        let response = client
            .post("/apikey/login")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);

        // editing
        let start = time::Instant::now();
        let response = client.get(format!("/questions/{}", 0)).dispatch();
        assert_eq!(response.status(), Status::Ok);

        let mut answers = vec![];
        answers.push((
            format!("answers.{}", 0),
            format!("new_answer_user_{}_lec_{}", u, 0),
        ));
        let postdata = serde_urlencoded::to_string(&answers).unwrap();
        info!(log, "Posting to questions for lec 0 answers {}", postdata);
        let response = client
            .post(format!("/questions/{}", 0)) // testing lecture 0 for now
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        edit_durations.push(start.elapsed());

        // delete account
        let start = time::Instant::now();
        let response = client.post("/delete").header(ContentType::Form).dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        delete_durations.push(start.elapsed());
    }

    /**********************************
     * anonymization
     ***********************************/
    // create all data again... because we just deleted them all lol
    if args.prime {
        let schema = std::fs::read_to_string(&args.schema).unwrap();
        match &args.connection {
            Connection::Port(port) => {
                let host = format!("127.0.0.1:{}", port);
                helpers::init_db(
                    false, // in-memory
                    &args.config.mysql_user,
                    &args.config.mysql_pass,
                    &host,
                    &args.class,
                    &schema,
                );
            }
            Connection::Socket(socket) => {
                helpers::init_db_with_socket(
                    false, // in-memory
                    &args.config.mysql_user,
                    &args.config.mysql_pass,
                    socket,
                    &args.class,
                    &schema,
                );
            }
        }
    }
    populate_db(
        &mut user2apikey,
        &mut account_durations,
        args,
        &client,
        &mut db,
        log.clone(),
    );

    // login as the admin
    let postdata =
        serde_urlencoded::to_string(&vec![("email", config::ADMIN.0), ("key", config::ADMIN.1)])
            .unwrap();
    let response = client
        .post("/apikey/login")
        .body(postdata)
        .header(ContentType::Form)
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    // anonymize
    let start = time::Instant::now();
    let response = client.post("/admin/anonymize").dispatch();
    anon_durations.push(start.elapsed());
    assert_eq!(response.status(), Status::SeeOther);

    print_stats(
        args,
        account_durations,
        anon_durations,
        leclist_durations,
        answers_durations,
        questions_durations,
        edit_durations,
        delete_durations,
        vec![],
        vec![],
        vec![],
        vec![],
        true,
    );
}

fn run_benchmark(args: &args::Args, rocket: Rocket<Build>) {
    let mut account_durations = vec![];
    let mut edit_durations = vec![];
    let mut delete_durations = vec![];
    let mut restore_durations = vec![];
    let mut anon_durations = vec![];
    let mut edit_durations_nonanon = vec![];
    let mut delete_durations_nonanon = vec![];
    let mut restore_durations_nonanon = vec![];
    let mut leclist_durations = vec![];
    let mut answers_durations = vec![];
    let mut questions_durations = vec![];

    let mut user2apikey = HashMap::new();
    let client = Client::tracked(rocket).expect("valid rocket instance");

    let mut db = match &args.connection {
        Connection::Port(port) => mysql::Conn::new(
            Opts::from_url(&format!(
                "mysql://{}:{}@127.0.0.1:{}/{}",
                args.config.mysql_user, args.config.mysql_pass, port, args.class
            ))
            .unwrap(),
        )
        .unwrap(),
        Connection::Socket(socket) => mysql::Conn::new(
            OptsBuilder::new()
                .socket(Some(socket))
                .user(Some(&args.config.mysql_user))
                .pass(Some(&args.config.mysql_pass))
                .db_name(Some(&args.class)),
        )
        .unwrap(),
    };

    let log = new_logger();
    populate_db(
        &mut user2apikey,
        &mut account_durations,
        args,
        &client,
        &mut db,
        log.clone(),
    );

    /************
     * admin read
     *************/
    {
        // login as the admin
        let postdata = serde_urlencoded::to_string(&vec![
            ("email", config::ADMIN.0),
            ("key", config::ADMIN.1),
        ])
        .unwrap();
        let response = client
            .post("/apikey/login")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);

        for _ in 0..NITERS {
            // admin read
            let start = time::Instant::now();
            let response = client.get(format!("/leclist")).dispatch();
            assert_eq!(response.status(), Status::Ok);
            leclist_durations.push(start.elapsed());

            // answers
            let start = time::Instant::now();
            let response = client.get(format!("/answers/{}", 0)).dispatch();
            assert_eq!(response.status(), Status::Ok);
            answers_durations.push(start.elapsed());
        }

        // logout
        let response = client.post("/apikey/logout").dispatch();
        assert_eq!(response.status(), Status::SeeOther);
    }

    /***********************************
     * editing nonanon data
     ***********************************/
    for u in 0..min(args.nusers, NITERS) {
        // login
        let email = format!("{}@mail.edu", u);
        let apikey = user2apikey.get(&email).unwrap();
        let postdata =
            serde_urlencoded::to_string(&vec![("email", &email), ("key", apikey)]).unwrap();
        let response = client
            .post("/apikey/login")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);

        // questions
        let start = time::Instant::now();
        let response = client.get(format!("/questions/{}", 0)).dispatch();
        assert_eq!(response.status(), Status::Ok);
        questions_durations.push(start.elapsed());

        // editing
        let start = time::Instant::now();
        let response = client.get(format!("/questions/{}", 0)).dispatch();
        info!(
            log,
            "Edit Public: Getting questions {}mus",
            start.elapsed().as_micros()
        );
        assert_eq!(response.status(), Status::Ok);

        let mut answers = vec![];
        answers.push((
            format!("answers.{}", 0),
            format!("new_answer_user_{}_lec_{}", u, 0),
        ));
        let postdata = serde_urlencoded::to_string(&answers).unwrap();
        info!(log, "Posting to questions for lec 0 answers {}", postdata);
        let post_start = time::Instant::now();
        let response = client
            .post(format!("/questions/{}", 0)) // testing lecture 0 for now
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        info!(
            log,
            "Edit Public: Posting questions {}mus",
            post_start.elapsed().as_micros()
        );
        assert_eq!(response.status(), Status::SeeOther);
        edit_durations_nonanon.push(start.elapsed());

        // logout
        let response = client.post("/apikey/logout").dispatch();
        assert_eq!(response.status(), Status::SeeOther);
    }

    /***********************************
     * gdpr deletion (no composition)
     ***********************************/
    let mut user2gdprdids = HashMap::new();
    for u in 0..min(args.nusers, NITERS) {
        let email = format!("{}@mail.edu", u);
        let apikey = user2apikey.get(&email).unwrap();

        // login as the user
        let postdata =
            serde_urlencoded::to_string(&vec![("email", &email), ("key", apikey)]).unwrap();
        let response = client
            .post("/apikey/login")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);

        info!(
            log,
            "User {} {} attempted to log in correctly, going to delete", email, apikey
        );
        let start = time::Instant::now();
        let response = client.post("/delete").header(ContentType::Form).dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        delete_durations_nonanon.push(start.elapsed());

        // get capabilities: GDPR deletion in this app doesn't produce anon records
        let file = File::open(format!("{}.{}", email, DID_FILE)).unwrap();
        let mut buf_reader = BufReader::new(file);
        let mut did = String::new();
        buf_reader.read_to_string(&mut did).unwrap();
        info!(log, "Got email {} with did {}", &email, did);
        user2gdprdids.insert(email.clone(), did);

        // logout
        let response = client.post("/apikey/logout").dispatch();
        assert_eq!(response.status(), Status::SeeOther);
    }

    /***********************************
     * gdpr restore (without composition)
     ***********************************/
    for u in 0..min(args.nusers, NITERS) {
        let email = format!("{}@mail.edu", u);
        let apikey = user2apikey.get(&email).unwrap();

        // login as the user
        // also logs out everyone else
        let postdata =
            serde_urlencoded::to_string(&vec![("email", &email), ("key", apikey)]).unwrap();
        let response = client
            .post("/apikey/login")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);

        let start = time::Instant::now();
        let did = user2gdprdids.get(&email).unwrap();
        let postdata =
            serde_urlencoded::to_string(&vec![("did", did), ("email", &email), ("apikey", apikey)])
                .unwrap();
        let response = client
            .post("/restore")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        restore_durations_nonanon.push(start.elapsed());

        let response = client.post("/apikey/logout").dispatch();
        assert_eq!(response.status(), Status::SeeOther);
    }

    /**********************************
     * anonymization
     ***********************************/
    // login as the admin
    {
        let postdata = serde_urlencoded::to_string(&vec![
            ("email", config::ADMIN.0),
            ("key", config::ADMIN.1),
        ])
        .unwrap();
        let response = client
            .post("/apikey/login")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);

        // anonymize
        let start = time::Instant::now();
        let response = client.post("/admin/anonymize").dispatch();
        anon_durations.push(start.elapsed());
        assert_eq!(response.status(), Status::SeeOther);

        // get records
        for u in 0..min(args.nusers, NITERS) {
            let email = format!("{}@mail.edu", u);

            // check results of anonymization: user has no answers
            for l in 0..args.nlec {
                let res = db
                    .query_iter(format!(
                        "SELECT answers.* FROM answers WHERE lec = {} AND email = '{}';",
                        l, email
                    ))
                    .unwrap();
                let mut rows = vec![];
                for row in res {
                    let rowvals = row.unwrap().unwrap();
                    let vals: Vec<Value> = rowvals.iter().map(|v| v.clone().into()).collect();
                    rows.push(vals);
                }
                assert_eq!(rows.len(), 0);
            }
        }
        // logout
        let response = client.post("/apikey/logout").dispatch();
        assert_eq!(response.status(), Status::SeeOther);
    }

    /***********************************
     * editing anonymized data
     ***********************************/
    for u in 0..min(args.nusers, NITERS) {
        let email = format!("{}@mail.edu", u);
        let apikey = user2apikey.get(&email).unwrap();

        let start = time::Instant::now();

        // render template to get edit creds
        let response = client.get(format!("/anon/auth")).dispatch();
        assert_eq!(response.status(), Status::Ok);
        info!(
            log,
            "Auth edit anon request: {}mus",
            start.elapsed().as_micros()
        );

        // set creds, post to edit anon's answer to lecture 0
        let anon_start = time::Instant::now();
        let postdata = serde_urlencoded::to_string(&vec![
            ("email", &email),
            ("apikey", apikey),
            ("lec_id", &format!("0")),
        ])
        .unwrap();
        let response = client
            .post("/anon/edit")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::Ok);
        info!(
            log,
            "Perform edit anon request: {}mus",
            anon_start.elapsed().as_micros()
        );

        // update answers to lecture 0
        let anon_start = time::Instant::now();
        let mut answers = vec![];
        answers.push((
            format!("answers.{}", 0),
            format!("new_answer_user_{}_lec_{}", u, 0),
        ));
        let postdata = serde_urlencoded::to_string(&answers).unwrap();
        let response = client
            .post(format!("/questions/{}", 0)) // testing lecture 0 for now
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        info!(
            log,
            "Update answers as anon request: {}mus",
            anon_start.elapsed().as_micros()
        );
        edit_durations.push(start.elapsed());

        // logged out
        let response = client.get(format!("/leclist")).dispatch();
        assert_eq!(response.status(), Status::Unauthorized);
    }
    // check answers for users for lecture 0
    // login as the admin
    let postdata =
        serde_urlencoded::to_string(&vec![("email", config::ADMIN.0), ("key", config::ADMIN.1)])
            .unwrap();
    let response = client
        .post("/apikey/login")
        .body(postdata)
        .header(ContentType::Form)
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);
    let res = db
        .query_iter("SELECT answers.* FROM answers WHERE lec = 0 AND q = 0;")
        .unwrap();
    let mut found_new = false;
    for row in res {
        let rowvals = row.unwrap().unwrap();
        info!(
            log,
            "Rowvals are {:?}",
            rowvals
                .iter()
                .map(|rv| from_value::<String>(rv.clone()))
                .collect::<Vec<String>>()
        );
        let answer: String = from_value(rowvals[3].clone());
        if answer.contains("new_answer") {
            found_new = true;
        }
    }
    assert!(found_new);

    // logout
    let response = client.post("/apikey/logout").dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    /***********************************
     * gdpr deletion (with composition)
     ***********************************/
    for u in 0..min(args.nusers, NITERS) {
        let email = format!("{}@mail.edu", u);
        let apikey = user2apikey.get(&email).unwrap();

        // login as the user
        let postdata =
            serde_urlencoded::to_string(&vec![("email", &email), ("key", apikey)]).unwrap();
        let response = client
            .post("/apikey/login")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);

        let start = time::Instant::now();
        let response = client.post("/delete").header(ContentType::Form).dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        delete_durations.push(start.elapsed());

        // update gdpr dids
        let file = File::open(format!("{}.{}", email, DID_FILE)).unwrap();
        let mut buf_reader = BufReader::new(file);
        let mut did = String::new();
        buf_reader.read_to_string(&mut did).unwrap();
        info!(log, "Got email {} with did {}", &email, did);
        user2gdprdids.insert(email.clone(), did);

        let response = client.post("/apikey/logout").dispatch();
        assert_eq!(response.status(), Status::SeeOther);
    }
    // login as the admin
    let postdata =
        serde_urlencoded::to_string(&vec![("email", config::ADMIN.0), ("key", config::ADMIN.1)])
            .unwrap();
    let response = client
        .post("/apikey/login")
        .body(postdata)
        .header(ContentType::Form)
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    // check results of delete: no answers or users exist
    let res = db.query_iter("SELECT * FROM answers;").unwrap();
    let mut rows = vec![];
    for row in res {
        let rowvals = row.unwrap().unwrap();
        let answer: String = from_value(rowvals[0].clone());
        rows.push(answer);
    }
    assert_eq!(
        rows.len(),
        args.nlec * args.nqs * (args.nusers - min(args.nusers, NITERS))
    );
    let res = db.query_iter("SELECT * FROM users;").unwrap();
    let mut rows = vec![];
    for row in res {
        let rowvals = row.unwrap().unwrap();
        let email: String = from_value(rowvals[0].clone());
        info!(log, "Got email {} from users after delete", email);
        rows.push(email);
    }
    // note: this gets messy because of pseudoprincipals
    //assert_eq!(rows.len()); // the admin
    let response = client.post("/apikey/logout").dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    /***********************************
     * gdpr restore (with composition)
     ***********************************/
    for u in 0..min(args.nusers, NITERS) {
        let email = format!("{}@mail.edu", u);
        let apikey = user2apikey.get(&email).unwrap();

        // login as the user
        let postdata =
            serde_urlencoded::to_string(&vec![("email", &email), ("key", apikey)]).unwrap();
        let response = client
            .post("/apikey/login")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);

        let start = time::Instant::now();
        let did = user2gdprdids.get(&email).unwrap();
        let postdata =
            serde_urlencoded::to_string(&vec![("did", did), ("email", &email), ("apikey", apikey)])
                .unwrap();
        let response = client
            .post("/restore")
            .body(postdata)
            .header(ContentType::Form)
            .dispatch();
        assert_eq!(response.status(), Status::SeeOther);
        restore_durations.push(start.elapsed());

        let response = client.post("/apikey/logout").dispatch();
        assert_eq!(response.status(), Status::SeeOther);
    }

    // database is back in anonymized form
    // check answers for lecture 0
    // login as the admin
    let postdata =
        serde_urlencoded::to_string(&vec![("email", config::ADMIN.0), ("key", config::ADMIN.1)])
            .unwrap();
    let response = client
        .post("/apikey/login")
        .body(postdata)
        .header(ContentType::Form)
        .dispatch();
    assert_eq!(response.status(), Status::SeeOther);

    let res = db
        .query_iter("SELECT * FROM answers WHERE lec = 0 AND q = 0;")
        .unwrap();
    let mut rows = vec![];
    let mut found_new = false;
    for row in res {
        let rowvals = row.unwrap().unwrap();
        let answer: String = from_value(rowvals[3].clone());
        if answer.contains("new_answer") {
            found_new = true;
        }
        rows.push(answer);
    }
    assert!(found_new);
    assert_eq!(rows.len(), args.nusers as usize);

    let res = db.query_iter("SELECT * FROM users;").unwrap();
    let mut rows = vec![];
    for row in res {
        let rowvals = row.unwrap().unwrap();
        let answer: String = from_value(rowvals[0].clone());
        rows.push(answer);
    }
    assert_eq!(
        rows.len(),
        1 + args.nusers as usize * (args.nlec as usize + 1)
    );

    print_stats(
        args,
        account_durations,
        anon_durations,
        leclist_durations,
        answers_durations,
        questions_durations,
        edit_durations,
        delete_durations,
        restore_durations,
        edit_durations_nonanon,
        delete_durations_nonanon,
        restore_durations_nonanon,
        false,
    );
}

fn print_stats(
    args: &args::Args,
    account_durations: Vec<Duration>,
    anon_durations: Vec<Duration>,
    leclist_durations: Vec<Duration>,
    answers_durations: Vec<Duration>,
    questions_durations: Vec<Duration>,
    edit_durations: Vec<Duration>,
    delete_durations: Vec<Duration>,
    restore_durations: Vec<Duration>,
    edit_durations_nonanon: Vec<Duration>,
    delete_durations_nonanon: Vec<Duration>,
    restore_durations_nonanon: Vec<Duration>,
    is_baseline: bool,
) {
    let suffix = if args.proxy { "_proxy" } else { "" };
    let filename = if is_baseline {
        format!(
            "../../../results/websubmit_results/disguise_stats_{}lec_{}users_baseline{}.csv",
            args.nlec, args.nusers, suffix
        )
    } else if args.dryrun {
        format!(
            "../../../results/websubmit_results/disguise_stats_{}lec_{}users_dryrun{}.csv",
            args.nlec, args.nusers, suffix
        )
    } else {
        format!(
            "../../../results/websubmit_results/disguise_stats_{}lec_{}users{}.csv",
            args.nlec, args.nusers, suffix
        )
    };

    // print out stats
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&filename)
        .unwrap();
    writeln!(
        f,
        "{}",
        account_durations
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        anon_durations
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        leclist_durations
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        answers_durations
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        questions_durations
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        edit_durations
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        delete_durations
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        restore_durations
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        edit_durations_nonanon
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        delete_durations_nonanon
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
    writeln!(
        f,
        "{}",
        restore_durations_nonanon
            .iter()
            .map(|d| d.as_micros().to_string())
            .collect::<Vec<String>>()
            .join(",")
    )
    .unwrap();
}
