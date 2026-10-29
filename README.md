# WeKnora Desktop

腾讯 [WeKnora](https://github.com/Tencent/WeKnora) 知识库的本地部署 + 桌面壳:

- **后端**: WeKnora **Lite** 单二进制(零外部依赖:SQLite + 内置 FTS5/sqlite-vec 检索 + 本地文件存储),由本项目源码构建
- **前端**: React 19 + TypeScript + Vite(自研控制台)
- **桌面容器**: Tauri 2 + Rust —— 负责服务进程生命周期、健康检查、日志流、系统托盘与工作台窗口

> 说明:WeKnora 官方桌面版基于 Wails(Go 壳)。本项目按需求改用 Tauri 2 + React/TS 重新实现壳层,复用官方 Lite 二进制作为后端。

## 功能

| 模块 | 说明 |
| --- | --- |
| 控制台 | 服务启动/停止/重启、健康状态与延迟、实时日志流(内存环形缓冲 + 文件) |
| 工作台 | 在独立 Tauri 窗口打开 WeKnora 官方 Vue 界面(顶层加载,不受 X-Frame-Options 限制),也可在浏览器打开 |
| 设置 | 监听地址/端口、自动启动、退出保活、Ollama 地址、附加环境变量、部署诊断 |
| 托盘 | 常驻托盘菜单:显示主窗口、启停服务、打开工作台、退出 |

## 目录结构

```
weknora-desktop/
├── src/                  # React + TS 前端
├── src-tauri/            # Tauri 2 + Rust 壳
│   └── src/
│       ├── lib.rs        # 命令注册、托盘、自动启动、退出清理
│       ├── service.rs    # Lite 进程管理 / 健康探测 / 日志采集
│       └── config.rs     # 配置持久化(app_config_dir/config.json)
├── runtime/              # 部署目录(linkmind-lite.exe + web/ + config/ + migrations/ + docreader/)
├── scripts/assemble-runtime.sh   # 从 WeKnora 仓库组装 runtime/(Lite 二进制与界面)
├── scripts/assemble-docreader.sh # 组装 docreader 文档解析运行时到 runtime/docreader/
└── tools/genicon/        # 图标生成器(Go)
```

## 分发(U 盘便携版)

```bash
npm run tauri build   # 产出 release exe 与 NSIS 安装包
# 组装便携包: portable/ = WeKnoraDesktop.exe + runtime/(不含 data/) + 预装运行时 + 使用说明
```

- `portable\` 整个文件夹拷入 U 盘即可交付;数据随 `runtime\data\` 走,备份即拷贝
- 便携包自带 docreader 文档解析运行时(`runtime\docreader\`,约 500MB):文字版
  PDF/Word/PPT/Excel 等开箱即解析,无需对方电脑安装 Python/Java
- `src-tauri\target\release\bundle\nsis\*.exe` 为安装版(自动带资源与 WebView2 引导)
- 便携版运行依赖 WebView2 / VC++ 运行库(预装运行时目录内有离线安装包)

## 从源码构建

### 1. 构建 WeKnora Lite 二进制(Windows)

依赖:Go ≥ 1.26、MinGW-w64 GCC(CGO)、Node.js ≥ 20。

```bash
cd WeKnora
cp frontend/dist -> web   # 若未构建前端: cd frontend && npm ci && npm run build && cd ..
export PATH="/d/tools/go/bin:/d/tools/mingw64/bin:$PATH"
export CGO_ENABLED=1 EDITION=lite
export CGO_CFLAGS="-Wno-deprecated-declarations -ID:/tools/sqlite-include"
# 必须用 ldflags 注入 EDITION=lite,否则静态页不注册(工作台 401)
export EDITION=lite GOPROXY="https://goproxy.cn,direct"
LDFLAGS="$(./scripts/get_version.sh ldflags) -X 'google.golang.org/protobuf/reflect/protoregistry.conflictPolicy=warn'"
# anydoc 标签必加(否则 .docm/.rtf/.odt 等 7 种格式无引擎);
# -extldflags=-lntdll 是 anydoc Rust 库在 MinGW 下链接所需
go build -tags "sqlite_fts5 anydoc" -ldflags="-w -s $LDFLAGS -extldflags=-lntdll" -o linkmind-lite.exe ./cmd/server
```

> Windows 无系统 sqlite3.h,`sqlite-vec` 的 CGO 绑定需要它;本方案将 mattn/go-sqlite3 的 amalgamation 头复制到 `D:/tools/sqlite-include/sqlite3.h` 提供给 CGO。

### 2. 组装 runtime 部署目录

```bash
bash scripts/assemble-runtime.sh    # 默认取 ../WeKnora,可传参指定仓库路径
bash scripts/assemble-docreader.sh  # 同上;需要 uv,产出 ~500MB 的便携解析运行时
```

docreader 运行时布局(由 Rust 壳自动发现并拉起,监听 127.0.0.1:50051):

```
runtime/docreader/
├── python/      # python-build-standalone CPython 3.13 整体拷贝(可任意搬移),
│                # 依赖已装入 python/Lib/site-packages
└── docreader/   # WeKnora 仓库 docreader 包源码(python -m docreader.main)
```

### 3. 构建桌面应用

```bash
npm install
npm run tauri dev      # 开发调试
npm run tauri build    # 产出 NSIS 安装包(src-tauri/target/release/bundle/)
```

打包时 `runtime/` 整体作为资源进入安装包(tauri.conf.json `bundle.resources`),Rust 侧按 `配置路径 → 资源目录 binaries/ → runtime/` 顺序解析。

## 运行时行为

- 配置存于 `%APPDATA%/com.knowlege.weknora.desktop/config.json`;首次启动自动生成 `SYSTEM_AES_KEY` / `JWT_SECRET`
- Lite 进程环境由壳注入(`DB_DRIVER=sqlite`、`STREAM_MANAGER_TYPE=memory`、`SERVER_PORT`、`DOCREADER_ADDR` 等),`extra_env` 可覆盖任意变量
- 服务启动时自动拉起 docreader 解析服务(设置可关;目录自动取部署目录内 `docreader/`,或设置中显式指定),并注入 `DOCREADER_ADDR=127.0.0.1:50051`
- 服务进程随应用退出而终止(可在设置中改为后台常驻)
- 健康检查:`GET /health`,启动窗口 180s,超时判定 Failed;运行中周期刷新延迟

## API 速览

服务就绪后:

- REST API:`http://127.0.0.1:8080/api/v1`(JWT 或 `X-API-Key`)
- API 文档:`http://127.0.0.1:8080/swagger/index.html`
- 官方 Web UI:工作台窗口,或直接访问 `http://127.0.0.1:8080`
