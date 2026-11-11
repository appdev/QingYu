# 独立笔记 API 服务

`qingyu-api` 是可单独运行的 HTTP 服务，供本机或远程 AI 工具访问服务端的笔记库。不需要启动桌面应用、WebView、Node.js 或图形环境。它和桌面端直接复用 `DiskLibrary` 的扫描、命名、路径校验、外部变化核对及事务保存，正文仍为 Markdown 文件。

本版本管理一个配置的笔记库。AI 的公开 `/api/v1` 接口只能通过文档和目录 ID 操作该库，不能指定服务器绝对路径；隐藏文件、符号链接和废纸篓笔记不通过公开 API 提供。服务读取的是已保存内容，不包含桌面编辑器的选区或未保存正文，也不自动连接桌面端的 S3 配置。

## Web 登录与 Docker 密码

同一镜像包含编译好的 Web 编辑器。设置 `QINGYU_WEB_PASSWORD` 即开启 Web，只有访问密码，没有账号；未设置时仍是原来的 API 服务，不公开编辑器页面。Web 模式不要求配置 API Token；需要 AI 直接调用时再单独设置 `QINGYU_API_TOKEN`。密码和 API Token 互不替代。

```sh
docker build -f Dockerfile.api -t qingyu-api .
docker volume create qingyu-notes
# 通过环境变量或 --env-file 注入至少 12 个字符的密码，不写入镜像。
docker run -d --name qingyu-api --restart unless-stopped \
  -p 127.0.0.1:8787:8787 \
  -e QINGYU_WEB_PASSWORD \
  -v qingyu-notes:/data qingyu-api
```

默认 Cookie 带 `Secure`，部署在 HTTPS 反向代理后，并让代理保留客户端 `Host`。服务使用 `Origin` 与 `https://Host` 核对登录、退出和 Cookie 写请求；不依赖客户端可伪造的转发头。Web 当前部署在域名根路径，不支持代理去除路径前缀。

仅在本机 HTTP 调试时增加 `-e QINGYU_WEB_SECURE_COOKIE=false`，随后访问 `http://127.0.0.1:8787`；同源检查会改用 `http://Host`。对外部署保持默认值并启用 HTTPS。

会话使用随机的 HttpOnly、SameSite=Strict Cookie，12 小时后过期，不向浏览器返回密码或 API Token，也不把它们存进 localStorage。退出会立即撤销当前会话；服务重启或更改密码并重建容器后，所有旧会话失效。每分钟全服务最多允许 10 次错误密码，超过后返回 429，最多等待一分钟；这是共享访问密码的统一限流。最多允许 256 个同时有效的会话。

未登录访问编辑器、设置或预览页面会跳转登录；未登录 API 请求返回 401。浏览器会话也可以访问已有 API，写操作必须同源；明确携带 Bearer Token 时始终按该 Token 的权限处理，只读 Token 不会借用浏览器会话获得写权限。编辑器提供“退出登录”按钮，主动退出前先保存当前草稿；输入法组字或保存未完成时等待后重试。页面恢复、聚焦和每分钟会检查会话状态；登录已过期时保留本机未提交草稿再返回登录页。

**由此服务托管的 Web 编辑器直接读写服务器的 `QINGYU_LIBRARY_DIR`（镜像默认 `/data`）。** 新建、编辑、重命名、置顶、移动、复制和笔记废纸篓/恢复共用桌面端的 `DiskLibrary` 事务；新建、重命名和移动目录复用现有目录操作。浏览器中的笔记选择和界面设置独立，不改变其他设备的选择。服务器 Web 不显示 S3、Finder、终端和系统文件夹废纸篓入口；当前不提供浏览器上传附件或服务器目录选择器。

浏览器不会自动上传或合并旧 localStorage 笔记。未提交内容按页面分别保留本机恢复草稿，正常保存只清除当前页面已确认的备份；其他标签页及旧版本草稿仍保留。连接中断或版本冲突时保留编辑、拒绝覆盖服务器；重新登录/打开时有下载草稿入口，可导出 Markdown 后手动对比。退出不会删除未提交草稿，共享电脑需要留意浏览器本地数据。普通 Vite/静态开发版仍使用浏览器笔记，不因访问过服务器版而迁移。

内部 `/api/web/library` 返回编辑器读取快照，`/api/web/commands` 每次只提交一个文档或目录操作；不接受客户端整库覆盖，也不接受绝对路径。笔记写入携带正文与元数据的版本摘要；目录重命名和移动检查整库版本，过期返回 412。内部接口只接受 Web 会话，不扩展公开 AI Token 权限。相对图片和附件按当前笔记路径读取，禁止越出笔记库、符号链接、隐藏文件和内部配置；显示地址不会写回 Markdown。正文更新在空闲、可见页面每 15 秒及重新聚焦时核对，未保存编辑期间不刷新覆盖。

当前编辑器一次加载整个个人笔记库的正文；大库尚未实现按文档懒加载。

### 使用宿主机挂载目录部署

仓库提供 `compose.yaml`，复用现有 `Dockerfile.api` 和同源服务。先准备**专用笔记目录**（不要使用账户主目录或密钥目录），并授予容器 UID/GID `10001:10001` 读写权限。Linux 新目录示例：

```sh
sudo install -d -o 10001 -g 10001 -m 0750 /srv/qingyu/notes
export QINGYU_DATA_DIR=/srv/qingyu/notes
# 通过安全的环境注入设置 QINGYU_WEB_PASSWORD，至少 12 个字符。
docker compose up -d --build
```

已有目录应按实际属主配置权限或 ACL，不要盲目递归修改其他应用的数据目录。Compose 要求宿主目录已存在，挂载到 `/data`，默认只监听宿主机 `127.0.0.1:8787`。正式部署保持 Secure Cookie 默认值，在根路径配置 HTTPS 反向代理；仅本机 HTTP 测试可临时设置 `QINGYU_WEB_SECURE_COOKIE=false`。升级镜像后继续挂载原目录即可；备份时保留整个目录，包括 `.miaoyan`、`Trash` 和附件，建议在服务停止后取得一致备份。不要同时运行多个容器写同一个笔记库。

非 Docker 部署可先执行 `npm ci`、`npm run build`，设置 `QINGYU_WEB_PASSWORD`、`QINGYU_WEB_DIR=/absolute/path/to/dist` 和 `QINGYU_LIBRARY_DIR`，再启动下文的二进制。静态目录必须只包含构建产物，不要指向笔记、源码或密钥目录。镜像已设置 `QINGYU_WEB_DIR=/app/web`。

访问控制由此服务执行；直接启动 Vite 开发服务器或用其他静态服务器发布 `dist` 不会自动获得密码保护，不能把这两种方式当作受保护的公网入口。

## 启动

在仓库根目录执行，Rust 版本沿用 `rust-toolchain.toml`：

```sh
export QINGYU_LIBRARY_DIR=/absolute/path/to/notes
export QINGYU_API_TOKEN="$(openssl rand -hex 32)"
export QINGYU_API_READ_TOKEN="$(openssl rand -hex 32)" # 可选，只允许 GET/HEAD
export QINGYU_API_ADDR=127.0.0.1:8787                # 默认值

cargo run --locked --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features server --bin qingyu-api
```

Token 必须是 32–4096 个无空格的可见 ASCII 字符；建议使用上述随机生成方式。纯 API 模式读写 Token 必填；Web 模式可以只配置访问密码。可选的只读 Token 必须与读写 Token 不同。Web 密码至少 12 个字符、最多 1024 个 UTF-8 字节，不允许全空白。配置无效时拒绝启动，不初始化笔记库。服务不会打印密码或 Token。

环境变量优先使用 `QINGYU_` 前缀；同名旧 `MIAOYAN_` 前缀仍可读取，已有部署可以继续启动。两者都设置时，新前缀优先。

镜像的目录和监听默认值沿用旧前缀，以便两种前缀的运行时配置都能覆盖默认值。已有 Docker 数据卷请继续挂载原卷；镜像更名不会迁移或重命名数据卷。

`QINGYU_LIBRARY_DIR` 不存在时会创建；已有目录直接扫描并建立 `.miaoyan` 索引，不覆盖已有正文。目录需要可写，因为扫描元数据和冲突检查也使用现有索引及锁。服务应使用专用笔记目录，持久化整个目录（包括 `.miaoyan`），不要挂载账户配置目录或存放密钥的目录。

独立发布只需构建并复制可执行文件到相同操作系统和架构的主机：

```sh
cargo build --locked --release --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features server --bin qingyu-api
# 输出：src-tauri/target/release/qingyu-api
```

服务收到 Ctrl+C 或 SIGTERM 后停止接受新连接，并等待现有请求结束。后台运行可交给主机的服务管理器。

## Docker 与远程访问

```sh
docker build -f Dockerfile.api -t qingyu-api .
docker volume create qingyu-notes
docker run -d --name qingyu-api --restart unless-stopped \
  -p 127.0.0.1:8787:8787 \
  -e QINGYU_API_TOKEN -e QINGYU_API_READ_TOKEN \
  -v qingyu-notes:/data qingyu-api
```

镜像以 UID/GID `10001:10001` 运行。改用宿主机目录挂载时，需要让该用户拥有目录读写权限。构建上下文由 `Dockerfile.api.dockerignore` 限定，不包含本地笔记、构建产物或 `.env`。未设置的可选只读 Token 不必传给容器。

远程部署时在服务前配置 HTTPS 反向代理，将其转发到 `127.0.0.1:8787`，本地 AI 使用 `https://你的域名/api/v1/...`。Token 通过 `Authorization` 请求头传递，不放入 URL。代理的请求体限制应至少为 4 MiB，并设置连接和请求超时；不要将携带 Bearer Token 的明文 HTTP 端口直接发布到公网。服务提供上文的共享访问密码，不提供多账号、用户隔离或浏览器跨域授权。

## 接口

AI 客户端的所有接口均要求：

```http
Authorization: Bearer <TOKEN>
```

已登录的同源浏览器也可以使用会话 Cookie 访问这些接口。

POST/PUT 另要求 `Content-Type: application/json`。请求 JSON 不接受未声明的字段，整个请求体上限为 4 MiB（包括 JSON 转义和字段开销）。响应带 `Cache-Control: no-store`。

| 方法与路径 | 行为 |
| --- | --- |
| `GET /api/v1/folders` | 返回所有目录，含根目录、空目录及 `parentId` |
| `GET /api/v1/notes` | 列表；可用 `folderId`、`cursor`、`limit`、`q` |
| `GET /api/v1/notes/{id}` | 返回单篇文档正文与版本，响应头含 `ETag` |
| `POST /api/v1/notes` | 创建文档，返回 `201`、文档、`ETag` 和 `Location` |
| `PUT /api/v1/notes/{id}` | 按 `If-Match` 更新正文，可同时重命名 |

目录响应示例：

```json
{"items":[{"id":"default","name":"备忘录","path":"","parentId":null}]}
```

列表返回 `{"items":[...],"nextCursor":null}`。每篇包含 `id`、`folderId`、`title`、库内相对 `path`、毫秒时间戳 `createdAt/updatedAt`、`readOnly`、`version` 和前 160 个字符的 `excerpt`；不返回全文、服务器根路径、当前选中项或同步元数据。单篇读取及写入响应在这些字段上增加 `source`。

`folderId` 只选择该目录直接包含的文档，不递归包含子目录；省略时列出全库。`q` 在标题和正文中做不区分大小写的子串匹配，可与目录过滤组合，不是正则表达式。`q` 最长 2000 字节，ID 和 cursor 最长 500 字节。

`limit` 默认为 50，范围 1–200。结果按 ID 升序排列，将 `nextCursor` 原样作为下一页的 `cursor`，其他查询参数保持不变，直到它为 `null`。分页是实时视图，不是跨请求快照；期间新建文档的 ID 可能位于已翻过的范围内，完整重扫时从第一页开始。

新建请求：

```json
{"folderId":"default","title":"AI 草稿","source":"# 标题\n\nMarkdown 正文\n"}
```

名称沿用桌面端规则：去首尾空白，冒号转换，斜杠作为名称字符处理，不作为目录路径；重名新建会分配 `名称 1.md` 等名称。API 额外拒绝空标题、超过 500 字符、反斜杠和隐藏名称。重命名发生同名冲突时拒绝覆盖。创建和更新不会主动切换桌面端当前选中的文档。

更新请求：

```http
PUT /api/v1/notes/<id>
Authorization: Bearer <读写 Token>
Content-Type: application/json
If-Match: "<读取响应中的 version>"

{"source":"# 修改后的正文\n","title":"可选的新标题"}
```

优先原样保存并回传读取响应头的 `ETag`，包括双引号。`If-Match` 必填且只接受一个完整版本，不接受 `*` 或弱 ETag。版本由文档内容和元数据计算，重启服务不会仅因重启改变版本；正文、名称等变化会使旧版本失效。两个调用者使用相同旧版本同时更新，最多一个成功。

外部程序改动也会在每次请求前通过现有核对逻辑载入。版本比较和提交在同一进程锁内执行；磁盘字节及跨进程索引变化再由存储层检查。仅原子替换文件不足以代替版本检查。所有成功写入均在事务提交后返回；文档被删除后不会因旧请求重新创建。损坏文档可读取但拒绝修改。

## 调用示例

以下例子在已配置上述环境变量的终端执行。远程调用时将 `API_BASE` 改为 HTTPS 地址，并为本地 AI 安全配置相同 Token。

```sh
API_BASE=http://127.0.0.1:8787
curl --fail-with-body -H "Authorization: Bearer $QINGYU_API_TOKEN" \
  "$API_BASE/api/v1/folders"
curl --fail-with-body -G -H "Authorization: Bearer $QINGYU_API_TOKEN" \
  --data-urlencode 'q=发布' --data-urlencode 'limit=20' \
  "$API_BASE/api/v1/notes"
curl --fail-with-body -H "Authorization: Bearer $QINGYU_API_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"folderId":"default","title":"AI 草稿","source":"# Hello\n"}' \
  "$API_BASE/api/v1/notes"
```

AI 工具先列目录或搜索，按文档 ID 读取，需要更新时携带该读取结果的 ETag。收到版本冲突必须重新读取并重新生成或合并修改，不能自动改成无条件覆盖。普通 HTTP 工具即可调用；本版本不附带 MCP 适配器。

## 错误与验证

业务错误使用 `{"error":{"code":"...","message":"..."}}`。参数、JSON 或请求体错误返回相应 4xx；不向客户端回传内部磁盘路径或原始存储错误。服务端错误详情写到 stderr。

| HTTP 状态 | 含义 |
| --- | --- |
| `401` | Token 缺失、错误或有多个 Authorization 头 |
| `403` | 只读 Token 尝试写入，或文档内容损坏不可写 |
| `404` | 目录/文档不存在、文档已在废纸篓，或接口不存在 |
| `409` | 保存时发生同名、磁盘并发修改、文件权限或其他存储冲突 |
| `412` | 文档版本已变化 |
| `413` | 请求体超出限制 |
| `428` | 更新未提供 If-Match |
| `503` | 笔记库无法核对，或暂被其他写入操作占用 |

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml \
  --no-default-features --features server --lib
```

测试复用真实临时目录，不连接用户笔记库或 S3，覆盖目录层级、分页搜索、Token 权限、读写、并发更新、外部修改/删除、重启版本、同名冲突、请求限制和路径边界。服务沿用现有整库内存扫描，适用于个人笔记库；未实现多租户、大库索引或全文检索引擎。
