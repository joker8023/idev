//! 本地 auth：无云端、无邮件、无 OAuth。
//! 原 Go: JWT(HS256) + 邮箱验证码(Resend/SMTP) + Google OAuth + mul_ PAT + daemon token。
//! 本地版：首次启动免密；用户在设置里设口令后，口令以 SHA-256 存 local_auth 表，
//! 前端每次启动调用 unlock(password) 解锁。无网络请求，无遥测。

use sha2::{Digest, Sha256};
use crate::db::Db;

pub fn hash_password(password: &str) -> String {
    let mut h = Sha256::new();
    h.update(password.as_bytes());
    hex::encode(h.finalize())
}

pub fn set_password(db: &Db, password: &str) -> anyhow::Result<()> {
    let hash = hash_password(password);
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO local_auth (singleton, password_hash) VALUES (1, ?1)
         ON CONFLICT(singleton) DO UPDATE SET password_hash = excluded.password_hash",
        [hash],
    )?;
    Ok(())
}

pub fn has_password(db: &Db) -> bool {
    let conn = db.0.lock().unwrap();
    let v: Option<Option<String>> = conn
        .query_row("SELECT password_hash FROM local_auth WHERE singleton = 1", [], |r| r.get(0))
        .ok();
    matches!(v, Some(Some(_)))
}

pub fn verify(db: &Db, password: &str) -> bool {
    if !has_password(db) {
        return true; // 未设口令 = 免密
    }
    let conn = db.0.lock().unwrap();
    let stored: Option<String> = conn
        .query_row("SELECT password_hash FROM local_auth WHERE singleton = 1", [], |r| r.get(0))
        .unwrap_or_default();
    stored.as_deref() == Some(&hash_password(password))
}
