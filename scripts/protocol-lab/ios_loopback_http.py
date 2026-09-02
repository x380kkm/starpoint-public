# audience: internal
# # ios-loopback-http
# 该模块将 Simulator 场景 HTTP 请求和重定向限制在数字 loopback 地址.

import base64
import hashlib
import re
import socket
from urllib.error import HTTPError, URLError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener


_HTTP_URL = re.compile(r"^https?://", re.IGNORECASE)


# //// 表示无法解析的 HTTP URL [@x380kkm 2026-08-21] ////
class InvalidHttpUrl(ValueError):
    pass


# //// 表示离开数字 loopback 的 HTTP URL [@x380kkm 2026-08-21] ////
class NonLoopbackUrl(ValueError):
    pass


# //// 规范化数字 loopback HTTP URL [@x380kkm 2026-08-21] ////
def normalize_base_url(value):
    if not isinstance(value, str):
        raise InvalidHttpUrl("HTTP URL is invalid")
    try:
        parsed = urlsplit(value)
        scheme = parsed.scheme.lower()
        hostname = parsed.hostname
        port = parsed.port
    except (TypeError, ValueError):
        raise InvalidHttpUrl("HTTP URL is invalid")
    if scheme not in ("http", "https"):
        raise InvalidHttpUrl("HTTP URL is invalid")
    if hostname not in ("127.0.0.1", "::1"):
        raise NonLoopbackUrl("HTTP URL is not loopback")
    host = "[::1]" if hostname == "::1" else hostname
    authority = host if port is None else "%s:%d" % (host, port)
    return "%s://%s" % (scheme, authority)


# //// 拒绝响应数据中的外部 HTTP URL [@x380kkm 2026-08-21] ////
def require_response_urls(value):
    pending = [value]
    visited = set()
    while pending:
        current = pending.pop()
        if isinstance(current, str) and _HTTP_URL.match(current):
            try:
                normalize_base_url(current)
            except (InvalidHttpUrl, NonLoopbackUrl):
                raise NonLoopbackUrl("response URL is not loopback")
            continue
        if isinstance(current, dict):
            identity = id(current)
            if identity in visited:
                continue
            visited.add(identity)
            pending.extend(current.keys())
            pending.extend(current.values())
        elif isinstance(current, (list, tuple)):
            identity = id(current)
            if identity in visited:
                continue
            visited.add(identity)
            pending.extend(current)


# //// 在重定向发生前验证目标仍为 loopback [@x380kkm 2026-08-21] ////
class _LoopbackRedirectHandler(HTTPRedirectHandler):
    def redirect_request(self, request, file_pointer, code, message, headers, new_url):
        try:
            normalize_base_url(new_url)
        except (InvalidHttpUrl, NonLoopbackUrl):
            raise NonLoopbackUrl("redirect URL is not loopback")
        return HTTPRedirectHandler.redirect_request(
            self,
            request,
            file_pointer,
            code,
            message,
            headers,
            new_url,
        )


_HTTP_OPENER = build_opener(_LoopbackRedirectHandler())


# //// 读取不含参数的响应内容类型 [@x380kkm 2026-08-21] ////
def _response_content_type(headers):
    return (headers.get("content-type") or "").split(";", 1)[0].strip().lower()


# //// 识别 urllib 包装的超时 [@x380kkm 2026-08-21] ////
def _is_timeout(error):
    if isinstance(error, (TimeoutError, socket.timeout)):
        return True
    return isinstance(error, URLError) and isinstance(
        error.reason,
        (TimeoutError, socket.timeout),
    )


# //// 构造一个限制在 loopback 的 HTTP 请求 [@x380kkm 2026-08-21] ////
def _build_request(base_url, method, target, body, content_type, headers):
    request_headers = dict(headers or {})
    if content_type is not None:
        request_headers["content-type"] = content_type
    if _HTTP_URL.match(target):
        try:
            normalize_base_url(target)
        except InvalidHttpUrl:
            raise NonLoopbackUrl("request URL is not loopback")
        request_url = target
    else:
        request_url = "%s%s" % (base_url, target)
    return Request(
        request_url,
        data=body,
        headers=request_headers,
        method=method,
    )


# //// 在有界生命周期内读取一个 loopback HTTP 响应 [@x380kkm 2026-08-21] ////
def _consume_response(http_request, timeout_seconds, consume):
    response = None
    try:
        try:
            response = _HTTP_OPENER.open(http_request, timeout=timeout_seconds)
        except HTTPError as error:
            response = error
        return consume(response)
    except Exception as error:
        if _is_timeout(error):
            raise TimeoutError("loopback request timed out")
        raise
    finally:
        if response is not None:
            response.close()


# //// 发送一个限制在 loopback 的 HTTP 请求 [@x380kkm 2026-08-21] ////
def request(base_url, timeout_seconds, method, target, body, content_type, headers=None):
    http_request = _build_request(
        base_url,
        method,
        target,
        body,
        content_type,
        headers,
    )
    return _consume_response(
        http_request,
        timeout_seconds,
        lambda response: (
            response.getcode(),
            _response_content_type(response.headers),
            response.read(),
        ),
    )


# //// 流式读取一个本地归档并计算摘要 [@x380kkm 2026-08-21] ////
def download(base_url, timeout_seconds, target, headers=None):
    http_request = _build_request(
        base_url,
        "GET",
        target,
        None,
        None,
        headers,
    )

    def consume(response):
        status = response.getcode()
        content_type = _response_content_type(response.headers)
        if not 200 <= status < 300:
            return status, content_type, 0, b"", ""
        digest = hashlib.sha256()
        signature = b""
        size = 0
        while True:
            chunk = response.read(1024 * 1024)
            if not chunk:
                break
            if len(signature) < 4:
                signature = (signature + chunk)[:4]
            digest.update(chunk)
            size += len(chunk)
        encoded_digest = base64.b64encode(digest.digest()).decode("ascii")
        return status, content_type, size, signature, encoded_digest

    return _consume_response(http_request, timeout_seconds, consume)
