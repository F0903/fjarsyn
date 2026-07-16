#[macro_export]
macro_rules! define_model {
    (
        $name:ident,
        $table:expr,
        fields: {
            $($field:ident : $type:ty),* $(,)?
        },
        create: {
            sql: $create_sql:expr,
            params: [ $($c_param:ident),* $(,)? ]
        },
        update: {
            sql: $update_sql:expr,
            params: [ $($u_param:ident),* $(,)? ]
        }
    ) => {
        #[derive(Debug, Clone, sqlx::FromRow, serde::Serialize, serde::Deserialize)]
        pub struct $name {
            pub id: i64,
            $(pub $field: $type,)*
            pub created_at: chrono::DateTime<chrono::Utc>,
            pub updated_at: chrono::DateTime<chrono::Utc>,
        }

        impl $name {
            pub async fn list(pool: &sqlx::SqlitePool) -> Result<Vec<Self>, $crate::Error> {
                sqlx::query_as::<_, Self>(concat!("SELECT * FROM ", $table, " ORDER BY id DESC"))
                    .fetch_all(pool)
                    .await
                    .map_err($crate::Error::DatabaseError)
            }

            pub async fn get_by_id(
                pool: &sqlx::SqlitePool,
                id: i64,
            ) -> Result<Option<Self>, $crate::Error> {
                sqlx::query_as::<_, Self>(concat!("SELECT * FROM ", $table, " WHERE id = ?"))
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err($crate::Error::DatabaseError)
            }

            pub async fn delete(pool: &sqlx::SqlitePool, id: i64) -> Result<(), $crate::Error> {
                let result = sqlx::query(concat!("DELETE FROM ", $table, " WHERE id = ?"))
                    .bind(id)
                    .execute(pool)
                    .await
                    .map_err($crate::Error::DatabaseError)?;
                if result.rows_affected() == 0 {
                    return Err($crate::Error::RecordNotFound { entity: $table, id });
                }
                Ok(())
            }

            pub async fn create(
                pool: &sqlx::SqlitePool,
                $($c_param: impl Into<$type>),*
            ) -> Result<Self, $crate::Error> {
                sqlx::query_as::<_, Self>($create_sql)
                    $(.bind($c_param.into()))*
                    .fetch_one(pool)
                    .await
                    .map_err($crate::Error::DatabaseError)
            }

            pub async fn update(
                pool: &sqlx::SqlitePool,
                id: i64,
                $($u_param: impl Into<$type>),*
            ) -> Result<Self, $crate::Error> {
                let updated = sqlx::query_as::<_, Self>($update_sql)
                    $(.bind($u_param.into()))*
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err($crate::Error::DatabaseError)?;
                updated.ok_or($crate::Error::RecordNotFound { entity: $table, id })
            }
        }
    };
}
