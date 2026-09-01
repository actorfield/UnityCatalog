//! An `ObjectLog` backed by S3 (or any API-compatible store, MinIO included).
//!
//! Built on the AWS SDK rather than hand-rolled HTTP so SigV4, retries and
//! endpoint resolution are not this module's problem. The only thing that
//! genuinely matters here is that `put_if_absent` maps onto a *conditional*
//! PutObject — `If-None-Match: *` — because the whole store design rests on it.
//! Both S3 (since August 2024) and MinIO support it.

use super::log::{ObjectLog, PutResult};
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use uc_errors::{ErrorCode, UcError};

pub struct S3Log {
    client: Client,
    bucket: String,
    /// Key prefix for this org, e.g. `acme/`. Always empty or slash-terminated.
    root: String,
}

fn s3_err(context: &str, e: impl std::fmt::Display) -> UcError {
    UcError::new(ErrorCode::Internal, format!("{context}: {e}"))
}

impl S3Log {
    /// `root` is the prefix every key sits under, e.g. the org slug. An empty
    /// root means the bucket itself.
    pub fn new(client: Client, bucket: impl Into<String>, root: impl Into<String>) -> Self {
        let mut root = root.into();
        if !root.is_empty() && !root.ends_with('/') {
            root.push('/');
        }
        Self {
            client,
            bucket: bucket.into(),
            root,
        }
    }

    /// Logical key -> physical S3 key.
    fn full(&self, key: &str) -> String {
        format!("{}{}", self.root, key)
    }

    /// Physical S3 key -> logical key. Callers parse versions out of these, so
    /// the root must be stripped or `version_from_key` sees a nested path and
    /// rejects every commit.
    fn strip(&self, key: &str) -> Option<String> {
        key.strip_prefix(self.root.as_str()).map(str::to_owned)
    }
}

#[async_trait::async_trait]
impl ObjectLog for S3Log {
    async fn put_if_absent(&self, key: &str, body: Vec<u8>) -> Result<PutResult, UcError> {
        let full = self.full(key);
        let result = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(&full)
            // The entire concurrency design is this one header. A backend that
            // ignores it would silently overwrite a commit and lose data with
            // no error anywhere.
            .if_none_match("*")
            .body(ByteStream::from(body))
            .send()
            .await;

        match result {
            Ok(_) => Ok(PutResult::Created),
            Err(e) => {
                // A lost race arrives as 412 Precondition Failed. S3 can also
                // answer 409 Conflict when concurrent conditional writes
                // collide, and that means the same thing: someone else has the
                // key. Anything else is a real error and must not be mistaken
                // for contention, or a broken bucket looks like a busy one.
                let status = match &e {
                    SdkError::ServiceError(se) => se.raw().status().as_u16(),
                    _ => 0,
                };
                if status == 412 || status == 409 {
                    Ok(PutResult::AlreadyExists)
                } else {
                    Err(s3_err(&format!("put {full}"), aws_display(&e)))
                }
            }
        }
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, UcError> {
        let full = self.full(key);
        match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&full)
            .send()
            .await
        {
            Ok(out) => {
                let bytes = out
                    .body
                    .collect()
                    .await
                    .map_err(|e| s3_err(&format!("read {full}"), e))?;
                Ok(Some(bytes.into_bytes().to_vec()))
            }
            Err(SdkError::ServiceError(se)) if se.err().is_no_such_key() => Ok(None),
            // A 404 that the SDK did not model as NoSuchKey — MinIO and some
            // gateways answer this way for a missing key.
            Err(SdkError::ServiceError(se)) if se.raw().status().as_u16() == 404 => Ok(None),
            Err(e) => Err(s3_err(&format!("get {full}"), aws_display(&e))),
        }
    }

    /// One page, as the trait allows. `log::list_all_after` drives it to
    /// exhaustion, so a large partition costs round trips rather than a
    /// silently short answer.
    async fn list_after(&self, prefix: &str, start_after: &str) -> Result<Vec<String>, UcError> {
        let out = self
            .client
            .list_objects_v2()
            .bucket(&self.bucket)
            .prefix(self.full(prefix))
            .start_after(self.full(start_after))
            .send()
            .await
            .map_err(|e| s3_err("list", aws_display(&e)))?;

        Ok(out
            .contents()
            .iter()
            .filter_map(|o| o.key())
            .filter_map(|k| self.strip(k))
            .collect())
    }

    async fn put(&self, key: &str, body: Vec<u8>) -> Result<(), UcError> {
        let full = self.full(key);
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&full)
            .body(ByteStream::from(body))
            .send()
            .await
            .map_err(|e| s3_err(&format!("put {full}"), aws_display(&e)))?;
        Ok(())
    }
}

/// SDK errors print almost nothing useful through Display; the service message
/// is the part worth surfacing.
fn aws_display<E: std::fmt::Debug, R: std::fmt::Debug>(e: &SdkError<E, R>) -> String {
    match e {
        SdkError::ServiceError(se) => format!("{:?}", se.err()),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    // Tests panic on purpose; see the note in the crate-level modules.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
    use super::*;

    fn log(root: &str) -> S3Log {
        // No network: these exercise key mapping only.
        let conf = aws_sdk_s3::Config::builder()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .build();
        S3Log::new(Client::from_conf(conf), "bucket", root)
    }

    #[test]
    fn root_is_slash_terminated() {
        assert_eq!(log("acme").full("_uc_log/x.json"), "acme/_uc_log/x.json");
        assert_eq!(log("acme/").full("_uc_log/x.json"), "acme/_uc_log/x.json");
        assert_eq!(log("").full("_uc_log/x.json"), "_uc_log/x.json");
    }

    /// Listed keys must come back logical. Leaving the root on would make every
    /// commit look like a nested path, and `version_from_key` rejects those --
    /// so replay would silently find no commits at all.
    #[test]
    fn listed_keys_are_stripped_back_to_logical_form() {
        let l = log("acme");
        let physical = "acme/_uc_log/00000000000000000007.json";
        let logical = l.strip(physical).unwrap();
        assert_eq!(logical, "_uc_log/00000000000000000007.json");
        assert_eq!(
            crate::store::action::version_from_key(&logical),
            Some(7),
            "a stripped key must still parse as a commit version"
        );
    }

    #[test]
    fn keys_outside_the_root_are_ignored() {
        assert_eq!(log("acme").strip("other-org/_uc_log/1.json"), None);
    }
}
