use aws_config;
use aws_types::region::Region;
use aws_sdk_s3::config::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::presigning::PresigningConfig;
use mlua::prelude::*;
use tokio::runtime::Runtime;
use std::time::Duration;
use std::path::Path;
use mlua;
use mlua::{ToLua, Value};
use uuid::Uuid;
use url::Url;

struct HeadResult {
    length: i64,
    content_type: Option<String>
}

impl<'lua> ToLua<'lua> for HeadResult {

    fn to_lua(self, lua: &'lua Lua) -> mlua::Result<Value<'lua>> {
        let length = Value::Integer(self.length);
        let content_type = match self.content_type {
            Some(typ) => Value::String(lua.create_string(&typ).unwrap()),
            None => Value::Nil
        };

        let result = lua.create_table()?;
        result.set("length", length)?;
        result.set("content_type", content_type)?;

        Ok(Value::Table(result))
    }

}

#[derive(Debug, Clone)]
struct ClientConfig {
    endpoint_url: Option<String>,
    bucket: String,
    base_domain: Option<String>,
    upload_path: String,
    region: String,
    accessid: String,
    accesskey: String
}

impl ClientConfig {
    fn new(endpoint_url: Option<&str>, id: &str, key: &str, region: &str,
        bucket: &str, base_domain: Option<&str>, upload_path: &str) -> ClientConfig {

        ClientConfig {
            endpoint_url: endpoint_url.map(|s| s.to_string()),
            bucket: bucket.to_string(),
            base_domain: base_domain.map(|s| s.to_string()),
            upload_path: upload_path.to_string(),
            region: region.to_string(),
            accessid: id.to_string(),
            accesskey: key.to_string()
        }
    }
}

impl<'lua> ToLua<'lua> for ClientConfig {

    fn to_lua(self, lua: &'lua Lua) -> mlua::Result<Value<'lua>> {
        let endpoint_url = match self.endpoint_url {
            Some(u) => Value::String(lua.create_string(&u)?),
            None => Value::Nil
        };
        let bucket = Value::String(lua.create_string(&self.bucket)?);
        let region = Value::String(lua.create_string(&self.region)?);
        let upload_path = Value::String(lua.create_string(&self.upload_path)?);
        let access_id = Value::String(lua.create_string(&self.accessid)?);
        let access_key = Value::String(lua.create_string(&self.accesskey)?);
        let base_domain = match self.base_domain {
            Some(domain) => Value::String(lua.create_string(&domain)?),
            None => Value::Nil
        };

        let result = lua.create_table()?;
        result.set("endpoint_url", endpoint_url)?;
        result.set("bucket", bucket)?;
        result.set("region", region)?;
        result.set("base_domain", base_domain)?;
        result.set("upload_path", upload_path)?;
        result.set("access_key", access_key)?;
        result.set("access_id", access_id)?;

        Ok(Value::Table(result))
    }

}

impl<'lua> FromLua<'lua> for ClientConfig {

    fn from_lua(value: Value<'lua>, _: &'lua Lua) -> mlua::Result<Self> {
        match value {
            Value::Table(t) => {
                let bucket: String = t.get("bucket")?;
                let region: String = t.get("region")?;
                let upload_path: String = t.get("upload_path")?;
                let base_domain: Option<String>  = match t.get("base_domain")? {
                    Value::String(s) => Some(s.to_str()?.to_string()),
                    Value::Nil => None,
                    _ => Err(mlua::Error::FromLuaConversionError {
                            from: "base_domain", to: "Option<String>",
                            message: Some("Domain must be a string or nil".to_string()) })?
                };
                let endpoint_url: Option<String>  = match t.get("endpoint_url")? {
                    Value::String(s) => Some(s.to_str()?.to_string()),
                    Value::Nil => None,
                    _ => Err(mlua::Error::FromLuaConversionError {
                            from: "endpoint_url", to: "Option<String>",
                            message: Some("Domain must be a string or nil".to_string()) })?
                };

                let accessid: String = t.get("access_id")?;
                let accesskey: String = t.get("access_key")?;
                Ok(ClientConfig::new(endpoint_url.as_deref(), &accessid, &accesskey,
                    &region, &bucket, base_domain.as_deref(), &upload_path))

            },
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(), to: "ClientConfig",
                message: Some("Cannot convert non Table value to ClientConfig".to_string()) })
        }

    }

}

#[derive(Debug)]
struct S3Client {
    _rt: Runtime,
    _client: Client,
    bucket: String,
    base_domain: Option<String>,
    upload_path: String,
}


impl S3Client {

    fn from_client_config(config: ClientConfig) -> Result<S3Client, String> {

        let runtime = Runtime::new().unwrap();
        let credentials = Credentials::new(&config.accessid, &config.accesskey, None, None, "");
        let region = config.region;
        let s3conf = match &config.endpoint_url {
            Some(url) => runtime.block_on(aws_config::from_env()
                        .credentials_provider(credentials)
                        .endpoint_url(url)
                        .region(Region::new(region)).load()),
            None => runtime.block_on(aws_config::from_env()
                        .credentials_provider(credentials)
                        .region(Region::new(region)).load())
        };

        let client = Client::new(&s3conf);

        Ok(S3Client{ _rt: runtime, _client: client, bucket: config.bucket,
            base_domain: config.base_domain, upload_path: config.upload_path })

    }

    fn list_files(&self) {
        let client = &self._client;
        let resp = self._rt.block_on(
            client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&self.upload_path)
                .send()).unwrap();

        for obj in resp.contents().unwrap_or_default() {
            println!("{}", obj.key().unwrap_or_default())
        }
    }

    fn put_presigned(&self, path: &str, filesize: i64) -> Result<Url, String> {
        let expires_in = Duration::from_secs(300u64);
        let client = &self._client;
        let target = Path::new(&self.upload_path).join(path);

        match self._rt.block_on(
            client
                .put_object()
                .bucket(&self.bucket)
                .key(target.to_str().unwrap())
                .content_length(filesize)
                .presigned(PresigningConfig::expires_in(expires_in).unwrap())) {
            Ok(req) => { Ok(Url::parse(&req.uri().to_string()).unwrap()) },
            Err(e) => { Err(format!("Could not generate put request: {}", e)) }

        }
    }

    fn create_upload_request(&self, filename: &str, filesize: i64) -> (Option<String>, Option<String>) {

        let mut uuidbuffer = Uuid::encode_buffer();
        let random = Uuid::new_v4().as_hyphenated().encode_lower(&mut uuidbuffer);

        let mut raw_get_url = String::new();
        raw_get_url.push_str("https://");
        match &self.base_domain {
            Some(d) => {
                raw_get_url.push_str(&d);
                raw_get_url.push_str("/");
            },
            None => {}
        }
        raw_get_url.push_str(&self.upload_path);
        raw_get_url.push_str("/");
        raw_get_url.push_str(random);
        raw_get_url.push_str("/");
        raw_get_url.push_str(filename);

        let get_url = Url::parse(&raw_get_url).unwrap();

        let put_url_url: Option<Url> = self.put_presigned(
            Path::new(random).join(filename).as_path().to_str().unwrap(), filesize)
            .ok();

        let put_url = match put_url_url {
            Some(url) => Some(url.to_string()),
            None => None
        };

        return (Some(get_url.to_string()), put_url)

    }

    fn check_exists(&self, path: &str) -> Option<HeadResult> {
        let target = Path::new(&self.upload_path).join(path);

        let client = &self._client;

        return match self._rt.block_on(
            client
                .head_object()
                .bucket(&self.bucket)
                .key(target.to_str().unwrap())
                .send()) {
            Ok(resp) => { Some(
                HeadResult { length: resp.content_length, content_type: resp.content_type }
            )},
            Err(_) => { None }

        }
    }
}

fn create_upload_request(_: &Lua, (filename, filesize, config): (String, i64, ClientConfig))
    -> LuaResult<(Option<String>, Option<String>)> {

    let client = S3Client::from_client_config(config);
    match client {
        Ok(c) => Ok(c.create_upload_request(&filename, filesize)),
        Err(_) => Err(mlua::Error::RuntimeError("Could not create upload request".to_string()))
    }
}

fn check_exists(_: &Lua, (filename, config): (String, ClientConfig)) -> LuaResult<Option<HeadResult>> {
    let client = S3Client::from_client_config(config);
    match client {
        Ok(c) => Ok(c.check_exists(&filename)),
        Err(_) => Err(mlua::Error::RuntimeError("Could not create upload request".to_string()))
    }
}

fn list_files(_: &Lua, config: ClientConfig) -> LuaResult<()> {
    let client = S3Client::from_client_config(config).unwrap();

    client.list_files();

    Ok(())
}

#[mlua::lua_module]
fn luas3put(lua: &Lua) -> LuaResult<LuaTable> {
    let exports = lua.create_table()?;
    exports.set("list_files", lua.create_function(list_files)?)?;
    exports.set("check_exists", lua.create_function(check_exists)?)?;
    exports.set("create_upload_request", lua.create_function(create_upload_request)?)?;
    Ok(exports)
}
