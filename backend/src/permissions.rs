use bitflags::bitflags;
use diesel::backend::Backend;
use diesel::deserialize::{self, FromSql, FromSqlRow};
use diesel::expression::AsExpression;
use diesel::pg::Pg;
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::Int4;
use serde::{Deserialize, Serialize};
use std::io::Write;

// Define your bitflags
//
// 注意：这里不要加 `#[serde(transparent)]`。bitflags 的 `serde` feature 会为生成的
// 类型注入自己的 Serialize/Deserialize，表示法是字符串（`"ADMIN | STAFF"`），
// `transparent` 完全不生效，只会误导读者以为它是整数表示。见本文件底部的测试。
bitflags! {
    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
        AsExpression, FromSqlRow
    )]
    #[diesel(sql_type = Int4)]
    pub struct Permissions: i32 {
        const NONE  = 0b0000_0000;
        const ADMIN = 0b0000_0001;
        const STAFF = 0b0000_0010;
    }
}

// Serialize for Diesel
impl ToSql<Int4, Pg> for Permissions
where
    i32: ToSql<Int4, Pg>,
{
    fn to_sql<'a>(&'a self, out: &mut Output<'a, '_, Pg>) -> serialize::Result {
        let value: i32 = self.bits();
        // For PostgreSQL, INT4 (i32) is expected in network byte order (big-endian).
        out.write_all(&value.to_be_bytes())?;
        Ok(IsNull::No)
    }
}

// Deserialize for Diesel
impl FromSql<Int4, Pg> for Permissions
where
    i32: FromSql<Int4, Pg>,
{
    fn from_sql(bytes: <Pg as Backend>::RawValue<'_>) -> deserialize::Result<Self> {
        let value = i32::from_sql(bytes)?;
        // 用 from_bits_retain 而不是 from_bits：以后新增一个 flag 并写进数据库时，
        // 还在跑旧代码的实例读到未知位不应该让每次查询都直接失败。
        Ok(Permissions::from_bits_retain(value))
    }
}

#[cfg(test)]
mod tests {
    use super::Permissions;

    #[test]
    fn serde_repr_is_flag_names_not_integers() {
        assert_eq!(
            serde_json::to_string(&Permissions::ADMIN).unwrap(),
            "\"ADMIN\""
        );
        assert_eq!(
            serde_json::to_string(&(Permissions::ADMIN | Permissions::STAFF)).unwrap(),
            "\"ADMIN | STAFF\""
        );

        // 整数表示必须被拒绝：否则客户端可以自己拼出任意组合位。
        assert!(serde_json::from_str::<Permissions>("1").is_err());
        assert!(serde_json::from_str::<Permissions>("3").is_err());

        // 未知 flag 名必须被拒绝——这就是 role 的白名单校验。
        assert!(serde_json::from_str::<Permissions>("\"SUPERUSER\"").is_err());

        assert_eq!(
            serde_json::from_str::<Permissions>("\"STAFF\"").unwrap(),
            Permissions::STAFF
        );
        assert_eq!(
            serde_json::from_str::<Permissions>("\"ADMIN | STAFF\"").unwrap(),
            Permissions::ADMIN | Permissions::STAFF
        );
    }

    #[test]
    fn permission_predicates() {
        let held = Permissions::ADMIN | Permissions::STAFF;

        // contains = ALL
        assert!(held.contains(Permissions::ADMIN));
        assert!(held.contains(Permissions::ADMIN | Permissions::STAFF));

        // intersects = ANY
        assert!(held.intersects(Permissions::STAFF));
        assert!(Permissions::ADMIN.intersects(Permissions::STAFF | Permissions::ADMIN));

        assert!(!Permissions::NONE.intersects(Permissions::ADMIN));
        assert!(!Permissions::STAFF.contains(Permissions::ADMIN));
        assert!(!Permissions::STAFF.intersects(Permissions::ADMIN));
    }

    #[test]
    fn bits_round_trip_and_retain_unknown() {
        for p in [
            Permissions::NONE,
            Permissions::ADMIN,
            Permissions::STAFF,
            Permissions::ADMIN | Permissions::STAFF,
        ] {
            assert_eq!(Permissions::from_bits_retain(p.bits()), p);
        }

        let unknown = Permissions::ADMIN.bits() | 0b1000_0000;
        assert_eq!(Permissions::from_bits_retain(unknown).bits(), unknown);
    }
}
