use chrono::{Datelike, NaiveDate, NaiveTime, Timelike, Weekday};
use std::{collections::HashMap, io::Read};

#[derive(Debug)]
enum Event {
    Once {
        title: String,
        begin: NaiveTime,
        end: NaiveTime,
        day: NaiveDate,
    },
    Recurring {
        title: String,
        begin: NaiveTime,
        end: NaiveTime,
        begin_recur: NaiveDate,
        end_recur: Option<NaiveDate>,
        recur_days: Vec<chrono::Weekday>,
    },
}

impl Event {
    fn begin(&self) -> &NaiveTime {
        match self {
            Self::Once { begin, .. } | Self::Recurring { begin, .. } => begin,
        }
    }
    fn end(&self) -> &NaiveTime {
        match self {
            Self::Once { end, .. } | Self::Recurring { end, .. } => end,
        }
    }
    fn title(&self) -> &str {
        match self {
            Self::Once { title, .. } | Self::Recurring { title, .. } => title,
        }
    }
}

impl std::fmt::Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let now = chrono::Local::now().naive_local().time();
        let delta = *self.begin() - now;
        let delta_text = if delta.num_minutes() < 60 {
            format!(
                "({} mins{})",
                delta.num_minutes(),
                if delta.num_minutes() != 1 { "s" } else { "" }
            )
        } else {
            format!(
                "({} hour{})",
                delta.num_hours(),
                if delta.num_hours() != 1 { "s" } else { "" }
            )
        };
        write!(
            f,
            "{:02}:{:02} - {:02}:{:02} {:<10} | {}",
            self.begin().hour(),
            self.begin().minute(),
            self.end().hour(),
            self.end().minute(),
            delta_text,
            self.title()
        )
    }
}

#[derive(Debug)]
struct CalError(String);
impl std::error::Error for CalError {}

impl std::fmt::Display for CalError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "CalError({})", self.0)
    }
}

#[derive(Debug)]
enum HeaderValue<'a> {
    One(&'a str),
    Many(Vec<&'a str>),
}

impl<'a> HeaderValue<'a> {
    fn one(&self) -> Option<&'a str> {
        match self {
            Self::One(s) => Some(s),
            _ => None,
        }
    }
    fn many(&self) -> Option<&Vec<&'a str>> {
        match self {
            Self::Many(v) => Some(v),
            _ => None,
        }
    }
}

fn parse_cal_file(contents: &str) -> Result<Event, Box<dyn std::error::Error>> {
    let mut in_header = false;
    let mut header_values = HashMap::<&str, HeaderValue>::new();
    let mut lines = contents.lines().peekable();

    while let Some(line) = lines.next() {
        if line == "---" {
            if in_header {
                // this means it's the end of the header, so we're done
                break;
            }
            // otherwise it must be the start of the header
            in_header = true;
            continue;
        }

        if in_header {
            let (key, value) = line.split_once(':').unwrap();

            // stupid special case for the one list so I don't have to use a full general yaml parser
            let header_value = if key == "daysOfWeek" {
                HeaderValue::Many(if value.is_empty() {
                    let mut days = Vec::new();
                    while let Some(next_line) =
                        lines.next_if(|next_line| next_line.trim_start().starts_with('-'))
                    {
                        let day =
                            next_line.trim_start_matches(|c: char| c.is_whitespace() || c == '-');
                        days.push(day);
                    }
                    days
                } else {
                    let start_bytes = value
                        .find('[')
                        .ok_or(CalError("Cannot find opening [ on list".into()))?
                        + 1;
                    let end_bytes = value
                        .find(']')
                        .ok_or(CalError("Cannot find closing ] on list".into()))?;
                    let without_brackets = &value[start_bytes..end_bytes];

                    // naive method of parsing a yaml list (should work for now)
                    without_brackets
                        .split(',')
                        .map(|x| x.trim_start())
                        .collect()
                })
            } else {
                HeaderValue::One(value.trim_start())
            };
            header_values.insert(key, header_value);
        }
    }
    let get_one = |name| {
        header_values
            .get(name)
            .ok_or(CalError(format!("Has no '{}", name)))?
            .one()
            .ok_or(CalError(format!("'{}' is a list", name)))
    };
    let get_many = |name| {
        header_values
            .get(name)
            .ok_or(CalError(format!("Has no '{}", name)))?
            .many()
            .ok_or(CalError(format!("'{}' is not a list", name)))
    };

    if get_one("type").unwrap_or("single") == "single" {
        Ok(Event::Once {
            title: get_one("title")?.into(),
            begin: get_one("startTime")?.parse()?,
            end: get_one("startTime")?.parse()?,
            day: get_one("date")?.parse()?,
        })
    } else {
        Ok(Event::Recurring {
            title: get_one("title")?.into(),
            begin: get_one("startTime")?.parse()?,
            end: get_one("endTime")?.parse()?,
            begin_recur: get_one("startRecur")?.parse()?,
            end_recur: get_one("endRecur").ok().map_or_else(
                || Ok::<Option<NaiveDate>, Box<dyn std::error::Error>>(None),
                |x| {
                    if x == "\"\"" {
                        Ok(None)
                    } else {
                        Ok(Some(x.parse()?))
                    }
                },
            )?,
            recur_days: get_many("daysOfWeek")?
                .iter()
                .map(|day| match *day {
                    "M" => Ok(Weekday::Mon),
                    "T" => Ok(Weekday::Tue),
                    "W" => Ok(Weekday::Wed),
                    "R" => Ok(Weekday::Thu),
                    "F" => Ok(Weekday::Fri),
                    "S" => Ok(Weekday::Sat),
                    "U" => Ok(Weekday::Sun),
                    _ => Err(CalError(format!("Unknown weekday '{}'", day))),
                })
                .collect::<Result<Vec<Weekday>, CalError>>()?,
        })
    }
}

fn parse_events(
    path: impl AsRef<std::path::Path>,
) -> Result<Vec<Event>, Box<dyn std::error::Error>> {
    std::fs::read_dir(path)?
        .filter(|x| {
            x.as_ref()
                .is_ok_and(|y| y.file_type().is_ok_and(|z| z.is_file()))
        })
        .map(|x| {
            let fname = x.unwrap().path();
            let mut file = std::fs::File::open(fname)?;
            let mut buffer = String::new();
            file.read_to_string(&mut buffer)?;
            parse_cal_file(&buffer)
        })
        .collect()
}

fn get_valid_events() -> Result<Vec<Event>, Box<dyn std::error::Error>> {
    let now = chrono::Local::now().naive_local();
    let mut events: Vec<Event> = std::env::args()
        .skip(1)
        .map(parse_events)
        .collect::<Result<Vec<Vec<Event>>, Box<dyn std::error::Error>>>()? // TODO can I avoid this `collect`?
        .into_iter()
        .flatten()
        .filter(|event| match event {
            Event::Once { day, begin, .. } => day == &now.date() && begin >= &now.time(),
            Event::Recurring {
                begin_recur,
                end_recur,
                recur_days,
                begin,
                ..
            } => {
                recur_days.contains(&now.date().weekday())
                    && &now.date() >= begin_recur
                    && end_recur.map(|day| now.date() <= day).unwrap_or(true)
                    && begin >= &now.time()
            }
        })
        .collect();
    events.sort_by(|a, b| a.begin().cmp(b.begin()));
    Ok(events)
}

fn main() {
    match get_valid_events() {
        Ok(events) => {
            for event in events {
                println!("{}", event)
            }
        }
        Err(e) => {
            eprintln!("Error processing event files: {}", e)
        }
    }
}
