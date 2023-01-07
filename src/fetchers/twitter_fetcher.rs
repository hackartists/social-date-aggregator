use std::{
    fs::File,
    thread::{self},
    time,
};

use csv::Writer;
use leaky_bucket::RateLimiter;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct TweetData {
    edit_history_tweet_ids: Vec<String>,
    text: String,
    created_at: String,
    id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Metadata {
    newest_id: String,
    oldest_id: String,
    result_count: u32,
    next_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TweetResponse {
    #[serde(default)]
    data: Option<Vec<TweetData>>,

    #[serde(default)]
    meta: Option<Metadata>,
}

pub struct TwitterFetcher {
    tag: String,
    from_date: u32,
    end_date: u32,
    cli: reqwest::blocking::Client,
    headers: HeaderMap,
    rate_limiter: RateLimiter,
}

impl TwitterFetcher {
    pub fn start(&self) {
        let mut year = self.from_date / 100;
        let mut month = self.from_date % 100;
        let end_year = self.end_date / 100;
        let end_month = self.end_date % 100;
        let is_last_month = |y: u32, m: u32| -> bool { y == end_year && m == end_month };
        println!("current: {}/{}", year, month);
        println!("end: {}/{}", end_year, end_month);

        while year <= end_year {
            while !is_last_month(year, month) && month <= 12 {
                self.fetch_month(year, month);
                month += 1;
            }
            month = 1;
            year += 1;
        }
    }

    async fn write_records(&self, file: &mut Writer<File>, data: Option<Vec<TweetData>>) {
        match data.is_none() {
            false => {
                for d in data.unwrap().as_slice() {
                    file.write_record(&[&d.id, &d.created_at, &d.text]);
                }
                file.flush();
            }

            true => {}
        }
    }

    fn fetch_month(&self, year: u32, month: u32) {
        let start_date = format!("{}-{:02}-01T00:00:00Z", year, month);
        let mut end_year = year;
        let mut end_month = month + 1;
        let mut next_token = String::new();
        match end_month {
            13 => {
                end_year += 1;
                end_month = 1;
            }
            _ => {}
        }

        let end_date = format!("{}-{:02}-01T00:00:00Z", end_year, end_month);

        let mut csv_file = Writer::from_path(format!("{}-{:02}.csv", year, month)).unwrap();
        let res = self.fetch(&start_date, &end_date, &next_token).unwrap();
        next_token = match res.meta.is_none() {
            true => String::from(""),
            false => res.meta.unwrap().next_token,
        };

        while !next_token.is_empty() {
            thread::sleep(time::Duration::from_secs(1));
            let res = self.fetch(&start_date, &end_date, &next_token).unwrap();
            next_token = match res.meta.is_none() {
                true => String::from(""),
                false => res.meta.unwrap().next_token,
            };
            match res.data.is_none() {
                false => {
                    for d in res.data.unwrap().as_slice() {
                        csv_file.write_record(&[&d.id, &d.created_at, &d.text]);
                    }
                    csv_file.flush();
                }

                true => {}
            };
        }

        println!("finished fetching {}-{:02} data", year, month);
    }

    fn fetch(
        &self,
        start_date: &String,
        end_date: &String,
        next_token: &String,
    ) -> Result<TweetResponse, Box<dyn std::error::Error>> {
        let  url =
            match next_token.is_empty() {
                true => format!("https://api.twitter.com/2/tweets/search/all?query={}&start_time={}&end_time={}&max_results=100&tweet.fields=id,text,edit_history_tweet_ids,created_at&user.fields=id,name,username,location", self.tag, start_date, end_date),
                false => format!("https://api.twitter.com/2/tweets/search/all?query={}&start_time={}&end_time={}&max_results=100&tweet.fields=id,text,edit_history_tweet_ids,created_at&user.fields=id,name,username,location&next_token={}", self.tag, start_date, end_date, next_token),
            };
        println!("{}", url);

        self.rate_limiter.acquire_one();
        let res: TweetResponse = self
            .cli
            .get(url)
            .headers(self.headers.clone())
            .send()?
            .json()
            .unwrap();

        Ok(res)
    }
}

pub fn new(token: &str, tag: &str, from_date: u32, end_date: u32) -> TwitterFetcher {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(AUTHORIZATION, HeaderValue::from_str(token).unwrap());

    let rate_limiter = RateLimiter::builder()
        .max(300)
        .refill(300)
        .interval(time::Duration::from_secs(60 * 15))
        .initial(300)
        .build();

    TwitterFetcher {
        tag: format!("%23{}", tag),
        from_date,
        end_date,
        cli: reqwest::blocking::Client::new(),
        headers,
        rate_limiter,
    }
}
