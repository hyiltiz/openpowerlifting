#![feature(plugin)]
#![plugin(rocket_codegen)]
#![feature(custom_derive)]

#![recursion_limit="256"] // For Diesel.
#[macro_use] extern crate diesel;
#[macro_use] extern crate diesel_infer_schema;

// For #[derive(Serialize)].
#[macro_use] extern crate serde_derive;

extern crate dotenv;
use std::env;

extern crate r2d2_diesel;
extern crate r2d2;

extern crate rocket;
extern crate rocket_contrib;

use rocket_contrib::Template;
use rocket::response::{NamedFile, Redirect};
use rocket::http::Status;
use rocket::{State};

use std::error::Error;
use std::path::{Path, PathBuf};

mod schema;
use schema::Entry;
use schema::Meet;
use schema::Lifter;
use schema::DbConn;

mod queries;
mod hbs;


struct DbStats {
    num_entries: i64,
    num_meets: i64,
}


enum StaticResult {
    Redirect(Redirect),
    NamedFile(NamedFile),
    NotFound,
}

impl<'r> rocket::response::Responder<'r> for StaticResult {
    fn respond_to(self, req: &rocket::Request) -> Result<rocket::Response<'r>, Status> {
        match self {
            StaticResult::Redirect(v) => v.respond_to(req),
            StaticResult::NamedFile(v) => v.respond_to(req),
            StaticResult::NotFound => Err(Status::NotFound)
        }
    }
}

#[get("/static/<file..>")]
fn static_handler(file: PathBuf) -> StaticResult {
    match env::var("USE_GITHUB_FOR_STATICS").is_ok() {
        true => {
            match file.to_str() {
                Some(v) => {
                    let github_url = format!("https://sstangl.github.io/openpowerlifting-static/{}", v);
                    let redirection = Redirect::to(&github_url);
                    StaticResult::Redirect(redirection)
                },
                None => StaticResult::NotFound
            }
        },
        false => {
            match NamedFile::open(Path::new("static/").join(file)).ok() {
                Some(v) => StaticResult::NamedFile(v),
                None => StaticResult::NotFound
            }
        }
    }
}


#[get("/index.html")]
fn index_html() -> Option<NamedFile> {
    NamedFile::open("htmltmp/index.html").ok()
}

#[get("/contact.html")]
fn contact_html() -> Option<NamedFile> {
    NamedFile::open("htmltmp/contact.html").ok()
}

#[get("/data.html")]
fn data_html() -> Option<NamedFile> {
    NamedFile::open("htmltmp/data.html").ok()
}

#[get("/faq.html")]
fn faq_html(stats: State<DbStats>) -> Option<Template> {
    let context = hbs::FaqContext {
        base: hbs::Base {
            title: "OpenPowerlifting FAQ",
            header: hbs::Header {
                num_entries: stats.num_entries,
                num_meets: stats.num_meets,
            }
        }
    };

    Some(Template::render("faq", context))
}


#[get("/meet.html")]
fn meet_html() -> Option<NamedFile> {
    NamedFile::open("htmltmp/meet.html").ok()
}

#[get("/meetlist.html")]
fn meetlist_html() -> Option<NamedFile> {
    NamedFile::open("htmltmp/meetlist.html").ok()
}

#[get("/")]
fn index() -> Option<NamedFile> {
    index_html()
}


// TODO: Don't use Box<Error> -- use a custom error type?
#[get("/meet/<meetpath..>")]
fn meet_handler(meetpath: PathBuf, conn: DbConn) -> Result<String, Box<Error>> {
    let meetpath_str = try!(meetpath.to_str().ok_or(
        std::io::Error::new(std::io::ErrorKind::Other, "Malformed string.")));

    let meet_option = queries::get_meet_by_meetpath(meetpath_str, &conn);
    if meet_option.is_none() {
        return Ok(String::from("Meet not found."));
    }
    let meet = meet_option.unwrap();

    let entries_option = queries::get_entries_by_meetid(meet.id, &conn);
    if entries_option.is_none() {
        return Ok(String::from("Error loading entries."));
    }
    let entries = entries_option.unwrap();

    let mut display = String::new();

    for entry in entries {
        display.push_str(&format!("{} - {}\n", entry.lifter_id, entry.sex));
    }

    Ok(display)
}

#[derive(FromForm)]
struct LiftersHtmlQueryParams {
    /// The lifter's full name.
    q: String,
}

#[get("/lifters.html?<params>")]
fn redirect_old_lifters_html(params: LiftersHtmlQueryParams, conn: DbConn)
    -> Result<Redirect, Status>
{
    let lifter: Lifter =
        queries::get_lifter_by_name_legacy(&params.q, &conn)
        .ok_or(Status::NotFound)?;

    let userpage = format!("/u/{}", lifter.username);
    Ok(Redirect::to(&userpage))
}

#[get("/u/<username>")]
fn lifter_handler(username: String, conn: DbConn) -> Result<Template, Status> {
    // Look up the Lifter by Username.
    let lifter: Lifter =
        queries::get_lifter_by_username(&username, &conn)
        .ok_or(Status::NotFound)?;

    // Look up all Entries corresponding to the Lifter.
    // Every lifter in the database has done at least one meet.
    let entries: Vec<(Entry, Meet)> =
        queries::get_entries_by_lifterid(lifter.id, &conn).ok_or(Status::NotFound)?;

    let mut best_raw_squat: f32 = 0.0;
    let mut best_raw_bench: f32 = 0.0;
    let mut best_raw_deadlift: f32 = 0.0;
    let mut best_raw_total: f32 = 0.0;
    let mut best_raw_wilks: f32 = 0.0;

    for entry in entries.iter() {
        if entry.0.equipment.is_raw_or_wraps() {
            best_raw_squat = best_raw_squat.max(entry.0.highest_squat());
            best_raw_bench = best_raw_bench.max(entry.0.highest_bench());
            best_raw_deadlift = best_raw_deadlift.max(entry.0.highest_deadlift());
            best_raw_total = best_raw_total.max(entry.0.totalkg.unwrap_or(0.0));
            best_raw_wilks = best_raw_wilks.max(entry.0.wilks.unwrap_or(0.0));
        }
    }

    let context = hbs::LifterContext {
        lifter_nameurl_html: &lifter.get_url(),
        entries: &entries,

        best_raw_squat:
            if best_raw_squat != 0.0 { Some(best_raw_squat) } else { None },
        best_raw_bench:
            if best_raw_bench != 0.0 { Some(best_raw_bench) } else { None },
        best_raw_deadlift:
            if best_raw_deadlift != 0.0 { Some(best_raw_deadlift) } else { None },
        best_raw_total:
            if best_raw_total != 0.0 { Some(best_raw_total) } else { None },
        best_raw_wilks:
            if best_raw_wilks != 0.0 { Some(best_raw_wilks) } else { None },

        base: hbs::Base {
            title: &lifter.name,

            header: hbs::Header {
                num_entries: 0, // TODO
                num_meets: 0, // TODO
            }
        }
    };

    Ok(Template::render("lifter", context))
}


fn rocket() -> rocket::Rocket {
    // Initialize an r2d2 database connection pool.
    let db_path = env::var("DATABASE_PATH").expect("DATABASE_PATH is not set.");
    let db_pool = schema::init_pool(&db_path);

    // Pre-cache some database information at boot.
    // Because the database is read-only, this information is correct
    // for the lifetime of the server.
    let conn = DbConn(db_pool.get().expect("Failed to get a connection from pool."));
    let num_entries = queries::count_entries(&conn).expect("Failed to count entries.");
    let num_meets = queries::count_meets(&conn).expect("Failed to count meets.");

    let db_stats = DbStats {
        num_entries: num_entries,
        num_meets: num_meets,
    };

    // Initialize the server.
    rocket::ignite()
        .manage(db_pool)
        .manage(db_stats)

        .mount("/", routes![index])
        .mount("/", routes![static_handler])
        .mount("/", routes![lifter_handler])
        .mount("/", routes![meet_handler])

        // Old HTML redirectors.
        .mount("/", routes![redirect_old_lifters_html])

        // Old HTML handlers.
        .mount("/", routes![index_html, contact_html, data_html, faq_html,
                            meet_html, meetlist_html])

        .attach(Template::fairing())
}


fn main() {
    // Populate std::env with the contents of any .env file.
    dotenv::from_filename("server.env").ok();

    // Run the server loop.
    rocket().launch();
}
