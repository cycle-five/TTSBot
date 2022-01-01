// Discord TTS Bot
// Copyright (C) 2021-Present David Thomas

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published
// by the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.

// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use std::{collections::HashMap, sync::Arc};

use async_recursion::async_recursion;
use dashmap::DashMap;
use strfmt::strfmt;

use deadpool_postgres::{tokio_postgres, Object as Connection};
use tokio_postgres::Statement;

use crate::constants::*;

#[poise::async_trait]
pub trait CacheKeyTrait {
    async fn get(&self, conn: Connection, stmt: Statement) -> Result<tokio_postgres::Row, Error>;
    async fn set_one(&self, conn: Connection, stmt: Statement, value: &(dyn tokio_postgres::types::ToSql + Sync)) -> Result<(), Error>;
    async fn create_row(&self, conn: Connection, stmt: Statement) -> Result<(), Error>;
    async fn delete(&self, conn: Connection, stmt: Statement) -> Result<(), Error>;
}

#[poise::async_trait]
impl CacheKeyTrait for u64 {
    async fn get(&self, conn: Connection, stmt: Statement) -> Result<tokio_postgres::Row, Error> {
        let row = match conn.query_opt(&stmt, &[&(self.to_owned() as i64)]).await? {
            Some(row) => row,
            None => 0.get(conn, stmt).await?, // ID 0 is default row
        };

        Ok(row)
    }
    async fn set_one(&self, conn: Connection, stmt: Statement, value: &(dyn tokio_postgres::types::ToSql + Sync)) -> Result<(), Error> {
        conn.execute(&stmt, &[&(self.to_owned() as i64), value]).await?;
        Ok(())
    }
    async fn create_row(&self, conn: Connection, stmt: Statement) -> Result<(), Error> {
        conn.execute(&stmt, &[&(self.to_owned() as i64)]).await?;
        Ok(())
    }
    async fn delete(&self, conn: Connection, stmt: Statement) -> Result<(), Error> {
        conn.execute(&stmt, &[&(self.to_owned() as i64)]).await?;
        Ok(())
    }
}

#[poise::async_trait]
impl CacheKeyTrait for [u64; 2] {
    async fn get(&self, conn: Connection, stmt: Statement) -> Result<tokio_postgres::Row, Error> {
        let [guild_id, user_id] = self;
        let row = match conn.query_opt(&stmt, &[&(guild_id.to_owned() as i64), &(user_id.to_owned() as i64)]).await? {
            Some(row) => row,
            None => [0, 0].get(conn, stmt).await?, // ID 0 is default row
        };

        Ok(row)
    }
    async fn set_one(&self, conn: Connection, stmt: Statement, value: &(dyn tokio_postgres::types::ToSql + Sync)) -> Result<(), Error> {
        let [guild_id, user_id] = self;
        conn.execute(&stmt, &[&(guild_id.to_owned() as i64), &(user_id.to_owned() as i64), value]).await?;
        Ok(())
    }
    async fn create_row(&self, conn: Connection, stmt: Statement) -> Result<(), Error> {
        let [guild_id, user_id] = self;
        conn.execute(&stmt, &[&(guild_id.to_owned() as i64), &(user_id.to_owned() as i64)]).await?;
        Ok(())
    }
    async fn delete(&self, conn: Connection, stmt: Statement) -> Result<(), Error> {
        let [guild_id, user_id] = self;
        conn.execute(&stmt, &[&(guild_id.to_owned() as i64), &(user_id.to_owned() as i64)]).await?;
        Ok(())
    }    
}

pub struct DatabaseHandler<T: CacheKeyTrait + std::cmp::Eq + std::hash::Hash> {
    pool: Arc<deadpool_postgres::Pool>,
    cache: DashMap<T, Arc<tokio_postgres::Row>>,

    single_insert: &'static str,
    create_row: &'static str,
    select: &'static str,
    delete: &'static str,
}

impl<T> DatabaseHandler<T>
where T: CacheKeyTrait + std::cmp::Eq + std::hash::Hash + std::marker::Sync + std::marker::Send
{
    pub fn new(
        pool: Arc<deadpool_postgres::Pool>,
        select: &'static str,
        delete: &'static str,
        create_row: &'static str,
        single_insert: &'static str,
    ) -> Result<Self, Error> {
        Ok(
            DatabaseHandler {
                pool, select, delete, create_row, single_insert,
                cache: DashMap::new(),
            }
        )
    }

    #[async_recursion]
    pub async fn get(&self, identifier: T) -> Result<Arc<tokio_postgres::Row>, Error> {
        if let Some(row) = self.cache.get(&identifier) {
            return Ok(row.clone());
        }

        let conn = self.pool.get().await?;
        let stmt = conn.prepare_cached(self.select).await?;
        let row = Arc::new(identifier.get(conn, stmt).await?);

        self.cache.insert(identifier, row.clone());
        Ok(row)
    }

    pub async fn create_row(
        &self,
        identifier: T
    ) -> Result<(), Error> {
        if self.cache.contains_key(&identifier) {
            return Ok(());
        }

        let conn = self.pool.get().await?;
        let stmt = conn.prepare_cached(self.create_row).await?;
        identifier.create_row(conn, stmt).await?;

        Ok(())
    }

    pub async fn set_one(
        &self,
        identifier: T,
        key: &str,
        value: &(dyn tokio_postgres::types::ToSql + Sync),
    ) -> Result<(), Error> {
        let conn = self.pool.get().await?;

        let stmt = conn.prepare_cached(&strfmt(self.single_insert, &{
            let mut kwargs: HashMap<String, String> = HashMap::new();
            kwargs.insert("key".to_string(), key.to_string());
            kwargs
        })?).await?;

        identifier.set_one(conn, stmt, value).await?;
        self.cache.remove(&identifier);

        Ok(())
    }

    pub async fn delete(&self, identifier: T) -> Result<(), Error> {
        let conn = self.pool.get().await?;

        let stmt = conn.prepare_cached(self.delete).await?;
        identifier.delete(conn, stmt).await?;
        self.cache.remove(&identifier);

        Ok(())
    }
}
