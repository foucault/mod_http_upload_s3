# Prosody S3 compatible HTTP upload

## Introduction

This [Prosody](https://prosody.im) module implements
[XEP-0363](https://xmpp.org/extensions/xep-0363.html) that enables clients to
upload files over HTTP on any S3 compatible storage that supports [AWS
Signature Version
4](https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-query-string-auth.html).
Instead of having the XMPP server as the upload (or download) gateway files are
uploaded directly to the object store using precomputed short-lived PUT
requests.

This is a fork with extra modifications of the original
[`mod_http_upload_s3`](https://github.com/abeluck/mod_http_upload_s3). The
key differences are:

* A Lua module, `luas3put`, has been written in Rust (with
  [mlua](https://docs.rs/mlua/latest/mlua/)) that wraps the official AWS SDK
  instead of constructing requests manually on the Lua side. This will allow
  (eventually) for more extensive support for standard AWS-like operations.

* Endpoint URLs can be customised that enables the use of other S3 compatible
  hosts. This library has been tested, for example, on both AWS S3 and
  Cloudfare R2.

* Instead of checking for `c2s` connections explicitly additional authorised
  uploaders can be specified (akin to
  [mod_http_file_share](https://prosody.im/doc/modules/mod_http_file_share)).

* You can specify a hostname to serve the files from. This avoids the need for
  public buckets and you can also use Cloudfront (or any other CDN) with your
  custom domain deployed in front of your bucket to facilitate caching (and
  more predictable URLs).

## Installation instructions

1. Compile with `cargo build --release` as usual
2. Rename the produced library `target/release/libluas3put.so`
   to `luas3put.so`.
3. Copy both `mod_http_upload_s3.lua` and `luas3put.so` into a
   directory that Prosody can load modules from.
4. Configure Prosody to use the module (see below)

Steps 1 and 2 can be done automatically on a clean Debian 11 image
by using the included podman script. Rename `podman` with `docker`
in `podman-build.sh` if you use docker instead.

You will probably have to adjust the CORS headers on wherever you
are serving the files from to allow `GET`, `HEAD`, `PUT` and
ideally also `OPTIONS` for all origins if you want access web-based
XMPP clients to work. Same with the `Authorization`, `Content-Type`
and `Content-Length` headers as they are mandated by XEP-0363.

## Prosody Configuration

Add a new `Component` to the Prosody configuration to handle the uploads.

```lua
Component "upload.xmpp.host" "http_upload_s3"
    name = "HTTP file upload"
    ssl = {
        certificate = "/etc/prosody/certs/upload.xmpp.host.crt";
        key = "/etc/prosody/certs/upload.xmpp.host.key";
    }
    -- Bucket region; use auto for Cloudflare
    http_upload_s3_region = "eu-west-2";
    -- Obvious
    http_upload_s3_access_id = "ACCESSID";
    -- Obvious
    http_upload_s3_secret_key = "SECRET";
    -- Optional; will default to S3 if not specified
    http_upload_s3_endpoint_url = "https://endpoint.url";
    -- Domain to serve files from, otherwise is the
    -- public bucket host
    http_upload_s3_base_domain = "download.xmpp.host";
    -- Name of your storage bucket
    http_upload_s3_bucket = "xmppbucket";
    -- Directory under bucket to store files; can be empty
    http_upload_s3_path = "uploads";
    -- Maximum file size; defaults to 100 MiB
    http_upload_s3_file_size_limit = 104857600
    -- Access list; add jids or hosts that you want to
    -- grant upload access in addition to the local hosts
    -- specified in Prosody configuration
    http_upload_s3_access = {
        "filesharingenthusiast@example.net", -- specific JID
        "example.org" -- anyone on @example.org
    }
```

## Compatibility

Should work with Prosody 0.10+. Tested on 0.11 and 0.12. Should also work with
trunk.

## Known issues

* Very rudimentary access control.
* No quotas or user restrictions.
* The lua module modifies `LUA_CPATH` to include the path where the lua module
  itself is located so that the shared library can be loaded. This is both
  awkward and might also bring in other modules which might not be desirable.
* Error handling on the Rust side could be better and the code could (should?)
  be neater overall.
* Needs more graceful handling of the unlikely situation of UUID clash.
* The S3 requests are forcefully converted into blocking using
  `tokio::runtime::Runtime::block_on`. This defeats the asynchronous character
  of the Rust AWS SDK but at the moment it is unclear how this can be improved.
  I wish I didn't have to drag in the whole Tokio runtime. Ideas to improve
  this are welcome!

## Alternatives

There are a ton of HTTP upload modules for Prosody and other XMPP servers. If
you want to store files locally you can use either `mod_http_file_share` for a
turnkey solution in Lua or `mod_http_upload_external` combined with something
like [prosody-filer](https://github.com/ThomasLeister/prosody-filer) to
offload this to an external component.
