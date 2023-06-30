-- mod_http_upload_s3
--
-- Copyright (C) 2023 Spyros Stathopoulos
-- Copyright (C) 2018 Abel Luck
-- Copyright (C) 2015-2016 Kim Alvefur
--
-- This file is MIT/X11 licensed.
--

local modpath = module:get_directory()

package.cpath = package.cpath .. ";" .. modpath .. "/?.so"

-- imports
local luas3put = require "luas3put";
local st = require "util.stanza";
local jid = require "util.jid";
local uuid = require"util.uuid".generate;
local http = require "util.http";
local dataform = require "util.dataforms".new;
local HMAC = require "util.hashes".hmac_sha256;
local SHA256 = require "util.hashes".sha256;

-- config
local file_size_limit = module:get_option_number(module.name .. "_file_size_limit",
	100 * 1024 * 1024); -- 10 MB
local aws_region = assert(module:get_option_string(module.name .. "_region"),
	module.name .. "_region is a required option");
local aws_bucket = assert(module:get_option_string(module.name .. "_bucket"),
	module.name .. "_bucket is a required option");
local aws_path = assert(module:get_option_string(module.name .. "_path"),
	module.name .. "_path is a required option");
local aws_access_id = assert(module:get_option_string(module.name .. "_access_id"),
	module.name .. "_aws_access_id is a required option");
local aws_secret_key = assert(module:get_option_string(module.name .. "_secret_key"),
	module.name .. "_aws_secret_key is a required option");
local aws_base_domain = module:get_option_string(module.name .. "_base_domain",
	nil);
local aws_endpoint_url = module:get_option_string(module.name .. "_endpoint_url",
	nil);
local access = module:get_option_set(module.name .. "_access", {});

-- Construct a table with the configuration that can be parsed by the rust module
local s3_config = {
    endpoint_url = aws_endpoint_url,
    bucket = aws_bucket,
    base_domain = aws_base_domain,
    upload_path = aws_path,
    region = aws_region,
    access_id = aws_access_id,
    access_key = aws_secret_key
}

-- depends
module:depends("disco");

-- namespace
local legacy_namespace = "urn:xmpp:http:upload";
local namespace = "urn:xmpp:http:upload:0";

-- identity and feature advertising
module:add_identity("store", "file", module:get_option_string("name", "HTTP File Upload"))
module:add_feature(namespace);
module:add_feature(legacy_namespace);

module:add_extension(dataform {
	{ name = "FORM_TYPE", type = "hidden", value = namespace },
	{ name = "max-file-size", type = "text-single" },
}:form({ ["max-file-size"] = tostring(file_size_limit) }, "result"));

module:add_extension(dataform {
	{ name = "FORM_TYPE", type = "hidden", value = legacy_namespace },
	{ name = "max-file-size", type = "text-single" },
}:form({ ["max-file-size"] = tostring(file_size_limit) }, "result"));

local function handle_request(uploader, origin, stanza, xmlns, filename, filesize, filetype)
	-- access control; reject the request if not at least one of these
	-- conditions are met
	-- (1) Access list is empty and host is one of the local hosts
	-- (2) Access list contains the uploader
	-- (3) Access list contains the uploader host
	local uploader_host = jid.host(uploader);
	if not ((access:empty() and prosody.hosts[uploader_host]) or
	         access:contains(uploader) or
			 access:contains(uploader_host)) then
		module:log("debug", "Failed request for upload slot from a %s", origin.type);
		origin.send(st.error_reply(stanza, "cancel", "not-authorized"));
		return nil, nil;
	end

	-- validate
	if not filename or filename:find("/") then
		module:log("debug", "Filename %q not allowed", filename or "");
		origin.send(st.error_reply(stanza, "modify", "bad-request", "Invalid filename"));
		return nil, nil;
	end
	-- request must contain a valid filesize
	if not filesize or filesize < 0 or filesize % 1 ~= 0 then
		module:log("debug", "Missing or invalid file size");
		origin.send(st.error_reply(stanza, "modify", "bad-request", "Missing or invalid file size"));
		return nil, nil;
	elseif filesize > file_size_limit then
		module:log("debug", "File too large (%d > %d)", filesize, file_size_limit);
		origin.send(st.error_reply(stanza, "modify", "not-acceptable", "File too large",
			st.stanza("file-too-large", {xmlns=xmlns})
				:tag("max-size"):text(tostring(file_size_limit))));
		return nil, nil;
	end

	local get_url, put_url = luas3put.create_upload_request(filename, filesize, s3_config)

	if not get_url or not put_url then
		module:log("debug", "PUT or GET urls not generated properly")
		return nil, nil
	end

	module:log("debug", "Handing out upload slot GET %s PUT %s to %s@%s [%d %s]",
		get_url, put_url, origin.username, origin.host, filesize, filetype);

	return get_url, put_url;
end

-- hooks
module:hook("iq/host/"..legacy_namespace..":request", function (event)
	local stanza, origin = event.stanza, event.origin;
	local request        = stanza.tags[1];
	local filename       = request:get_child_text("filename");
	local filesize       = tonumber(request:get_child_text("size"));
	local filetype       = request:get_child_text("content-type") or "application/octet-stream";
	local uploader       = jid.bare(stanza.attr.from)

	local get_url, put_url = handle_request(
		uploader, origin, stanza, legacy_namespace, filename, filesize, filetype);

	if not get_url then
		-- error was already sent
		return true;
	end

	local reply = st.reply(stanza)
		:tag("slot", { xmlns = legacy_namespace })
			:tag("get"):text(get_url):up()
			:tag("put"):text(put_url):up()
		:up();
	origin.send(reply);
	return true;
end);

module:hook("iq/host/"..namespace..":request", function (event)
	local stanza, origin = event.stanza, event.origin;
	local request        = stanza.tags[1];
	local filename       = request.attr.filename;
	local filesize       = tonumber(request.attr.size);
	local filetype       = request.attr["content-type"] or "application/octet-stream";
	local uploader       = jid.bare(stanza.attr.from)

	local get_url, put_url = handle_request(
		uploader, origin, stanza, namespace, filename, filesize, filetype);

	if not get_url then
		-- error was already sent
		return true;
	end

	local reply = st.reply(stanza)
		:tag("slot", { xmlns = namespace})
			:tag("get", { url = get_url }):up()
			:tag("put", { url = put_url }):up()
		:up();
	origin.send(reply);
	return true;
end);
