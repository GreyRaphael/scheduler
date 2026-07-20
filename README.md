# Scheduler — 轻量级任务调度器

一个基于 Rust + Axum + SQLite 的轻量级任务调度器，内置 Web UI，支持 Cron、一次性定时、间隔三种触发方式，支持命令执行和 Webhook 两种动作类型。

## 功能特性

- **三种触发方式**：Cron 表达式、一次性定时（Once）、固定间隔（Interval）
- **两种动作类型**：执行系统命令（Command）、发送 HTTP 请求（Webhook）
- **内置 Web UI**：任务管理、执行历史、调度器控制一站式操作
- **失败重试**：支持配置最大重试次数
- **超时控制**：每个任务可独立设置超时时间
- **Gotify 推送**：任务执行结果可通过 Gotify 推送通知
- **Token 认证**：可选的 API 访问认证
- **SQLite 存储**：零依赖，数据持久化

## 快速开始

### 安装

```bash
cargo build --release
```

### 配置

编辑 `config.toml`：

```toml
listen = "0.0.0.0:7070"
db = "./scheduler.db"
token = "your-secret-token"   # 留空则不启用认证
log_level = "info"
gotify_url = "http://your-gotify-server:8080/"  # 留空则不启用推送
```

也可以通过命令行参数或环境变量覆盖：

```bash
# 命令行参数
./scheduler --listen 0.0.0.0:8080 --db /data/scheduler.db --token mytoken

# 环境变量
export SCHEDULER_LISTEN=0.0.0.0:8080
export SCHEDULER_TOKEN=mytoken
./scheduler
```

### 运行

```bash
./scheduler
```

启动后访问 `http://localhost:7070` 即可使用 Web UI。

---

## 创建任务 — 触发方式详解

任务通过 Web UI 或 API 创建。每个任务需要指定 **触发方式（trigger_type + trigger_expr）** 和 **动作类型（action_type + action_config）**。

### 1. Cron 触发

使用标准 Cron 表达式定义周期性任务。表达式为 **6 位格式**（含秒字段）：

```
┌───────────── 秒 (0-59)
│ ┌───────────── 分 (0-59)
│ │ ┌───────────── 时 (0-23)
│ │ │ ┌───────────── 日 (1-31)
│ │ │ │ ┌───────────── 月 (1-12)
│ │ │ │ │ ┌───────────── 星期 (0-6, 0=周日)
│ │ │ │ │ │
* * * * * *
```

创建 Cron 任务时，可通过 `cron_tz_mode` 字段选择时区模式：

| cron_tz_mode | 说明 |
|---|---|
| `utc`（默认） | Cron 表达式按 UTC 时间解析 |
| `local` | Cron 表达式按服务器本地时间解析，自动转换为 UTC 存储 |

**示例 — 每天 UTC 8 点执行备份脚本：**

```json
{
  "name": "daily-backup",
  "trigger_type": "cron",
  "trigger_expr": "0 0 8 * * *",
  "cron_tz_mode": "utc",
  "action_type": "command",
  "action_config": {
    "program": "/usr/local/bin/backup.sh",
    "args": ["--full"],
    "working_dir": "/data"
  }
}
```

**示例 — 每天本地时间 9 点执行报告脚本（服务器在 UTC+8）：**

```json
{
  "name": "morning-report",
  "trigger_type": "cron",
  "trigger_expr": "0 0 9 * * *",
  "cron_tz_mode": "local",
  "action_type": "command",
  "action_config": {
    "program": "python",
    "args": ["report.py"]
  }
}
```

> 上例中 `cron_tz_mode: "local"` 会将 `0 0 9 * * *`（本地 9:00）自动转换为 `0 0 1 * * *`（UTC 1:00，假设服务器在 UTC+8）进行调度。

**常用 Cron 表达式示例：**

| trigger_expr | 含义 |
|---|---|
| `0 0 8 * * *` | 每天 8:00（取决于 cron_tz_mode） |
| `0 30 9 * * 1-5` | 每周一至周五 9:30 |
| `0 0 */2 * * *` | 每隔 2 小时 |
| `0 0 0 1 * *` | 每月 1 号 00:00 |
| `30 * * * * *` | 每分钟的第 30 秒 |

### 2. Once 触发（一次性定时）

指定一个 **RFC 3339 格式** 的未来时间点，任务在该时刻执行一次后自动禁用。

时间格式：`YYYY-MM-DDTHH:MM:SS+时区偏移`

| trigger_expr | 含义 |
|---|---|
| `2026-06-01T08:00:00Z` | UTC 时间 2026-06-01 08:00:00 |
| `2026-06-01T16:00:00+08:00` | 北京时间 2026-06-01 16:00:00 |
| `2026-12-31T23:59:00-05:00` | 美东时间 2026-12-31 23:59:00 |

**时区说明**：`+08:00` 表示 UTC+8（北京时间），`-05:00` 表示 UTC-5（美东时间），`Z` 表示 UTC。

**示例 — 在北京时间 2026-06-01 16:00 发送 Webhook 通知：**

```json
{
  "name": "scheduled-notify",
  "trigger_type": "once",
  "trigger_expr": "2026-06-01T16:00:00+08:00",
  "action_type": "webhook",
  "action_config": {
    "url": "https://hooks.example.com/trigger",
    "method": "POST",
    "body": "{\"msg\": \"定时任务已触发\"}"
  }
}
```

**示例 — 在 UTC 时间 2026-07-01 00:00 执行数据库迁移：**

```json
{
  "name": "db-migration",
  "trigger_type": "once",
  "trigger_expr": "2026-07-01T00:00:00Z",
  "action_type": "command",
  "action_config": {
    "program": "python",
    "args": ["migrate.py", "--apply"],
    "working_dir": "/app"
  }
}
```

> **注意**：Once 任务执行一次后自动设置为 `Completed` 并禁用，不会再次执行。

### 3. Interval 触发（固定间隔）

以任务创建/加载时刻为起点，每隔指定秒数执行一次。

创建 Interval 任务时，可通过 `interval_mode` 字段选择间隔模式：

| interval_mode | 说明 |
|---|---|
| `fixed_delay`（默认） | **固定延迟**：等待上次任务**执行完成**后，再开始计算下一个间隔时间。适用于执行耗时较长，或不能重叠执行的任务。 |
| `fixed_rate` | **固定频率**：忽略任务执行耗时，严格按照设定的间隔时间触发（例如每 60 秒触发一次，不管中间任务执行了多久）。如果任务执行时间超过了间隔时间，可能会导致任务并发或堆积。 |

| trigger_expr | 含义 |
|---|---|
| `60` | 每 60 秒（1 分钟） |
| `300` | 每 300 秒（5 分钟） |
| `3600` | 每 3600 秒（1 小时） |
| `86400` | 每 86400 秒（1 天） |

**示例 — 每 5 分钟检查服务健康状态：**

```json
{
  "name": "health-check",
  "trigger_type": "interval",
  "trigger_expr": "300",
  "action_type": "webhook",
  "action_config": {
    "url": "http://localhost:8080/health",
    "method": "GET"
  },
  "timeout_secs": 30
}
```

**示例 — 每小时清理临时文件：**

```json
{
  "name": "cleanup-temp",
  "trigger_type": "interval",
  "trigger_expr": "3600",
  "action_type": "command",
  "interval_mode": "fixed_delay",
  "action_config": {
    "program": "find",
    "args": ["/tmp", "-type", "f", "-mtime", "+1", "-delete"]
  }
}
```

---

## 动作类型详解

每个任务可以同时配置 **Command** 和 **Webhook**，两者都是可选的（至少填一个）：

- **只填 Command** → 仅执行命令
- **只填 Webhook** → 仅发送 HTTP 请求
- **都填** → 先执行 Command，再发送 Webhook

### Command（执行命令）

直接执行指定程序（不经过 Shell），通过 `program` 指定可执行文件路径，`args` 传递参数列表。

```json
{
  "command_config": {
    "program": "python",
    "args": ["script.py", "--verbose"],
    "env": {"API_KEY": "secret"},
    "working_dir": "/app"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `program` | string | 是 | 要执行的程序或脚本路径 |
| `args` | string[] | 否 | 命令参数列表 |
| `env` | object | 否 | 额外的环境变量 |
| `working_dir` | string | 否 | 工作目录 |

### Webhook（HTTP 请求）

发送 HTTP 请求到指定 URL。可用于调用 API、触发 Gotify 推送等。

```json
{
  "webhook_config": {
    "url": "https://api.example.com/webhook",
    "method": "POST",
    "headers": {"Content-Type": "application/json"},
    "body": "{\"event\": \"task_complete\"}"
  }
}
```

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `url` | string | 是 | 目标 URL |
| `method` | string | 否 | HTTP 方法，默认 `GET` |
| `headers` | object | 否 | 自定义请求头 |
| `body` | string | 否 | 请求体（自动添加 `Content-Type: application/json`） |

> **注意**：HTTP 状态码 >= 400 时任务视为失败。如果未手动设置 `Content-Type`，当有 body 时会自动添加 `application/json`。

### Webhook 模板变量

当 Command + Webhook 同时配置时，webhook body 支持以下模板变量，执行时自动替换：

| 变量 | 说明 |
|---|---|
| `{{task_name}}` | 任务名称 |
| `{{status}}` | Command 执行结果：`success` / `failed` |
| `{{exit_code}}` | Command 返回码（如 `0`、`1`） |
| `{{stdout}}` | Command 标准输出 |
| `{{stderr}}` | Command 标准错误输出 |

**示例 — 执行命令后将结果发送到 Gotify：**

```json
{
  "name": "backup-with-notify",
  "trigger_type": "cron",
  "trigger_expr": "0 0 2 * * *",
  "command_config": {
    "program": "/usr/local/bin/backup.sh"
  },
  "webhook_config": {
    "url": "http://gotify:8080/message?token=MyToken",
    "method": "POST",
    "body": "{\"title\":\"{{task_name}}\",\"message\":\"Status: {{status}}, Exit: {{exit_code}}\",\"priority\":5}"
  }
}
```

---

## 通用任务字段

| 字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| `name` | string | — | 任务名称（唯一） |
| `description` | string | `""` | 任务描述 |
| `trigger_type` | string | — | `cron` / `once` / `interval` |
| `trigger_expr` | string | — | 触发表达式（见上文） |
| `cron_tz_mode` | string | `utc` | Cron 时区模式：`utc` / `local` / `Asia/Shanghai` 等（仅 trigger_type=cron 时生效） |
| `interval_mode` | string | `fixed_delay` | 间隔模式：`fixed_delay` / `fixed_rate`（仅 trigger_type=interval 时生效） |
| `command_config` | object | — | Command 配置（见上文，与 webhook_config 至少填一个） |
| `webhook_config` | object | — | Webhook 配置（见上文，与 command_config 至少填一个） |
| `enabled` | bool | `true` | 是否启用 |
| `max_retries` | u32 | `0` | 失败后最大重试次数 |
| `timeout_secs` | u64 | `3600` | 执行超时（秒） |

---

## API 接口

所有接口前缀为 `/api/v1`。若配置了 `token`，需在请求头中携带：

```
Authorization: Bearer your-secret-token
```

### 任务管理

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/api/v1/tasks` | 任务列表（支持分页、筛选） |
| `POST` | `/api/v1/tasks` | 创建任务 |
| `GET` | `/api/v1/tasks/{id}` | 任务详情 |
| `PUT` | `/api/v1/tasks/{id}` | 更新任务 |
| `DELETE` | `/api/v1/tasks/{id}` | 删除任务 |
| `POST` | `/api/v1/tasks/{id}/enable` | 启用任务 |
| `POST` | `/api/v1/tasks/{id}/disable` | 禁用任务 |
| `POST` | `/api/v1/tasks/{id}/trigger` | 手动触发任务 |

**任务列表查询参数：**

| 参数 | 说明 |
|---|---|
| `page` | 页码（默认 1） |
| `per_page` | 每页条数（默认 20，最大 100） |
| `search` | 按名称/描述搜索 |
| `status` | 按状态筛选：`active` / `paused` / `completed` / `failed` |
| `trigger_type` | 按触发方式筛选：`cron` / `once` / `interval` |

**创建任务示例：**

```bash
curl -X POST http://localhost:7070/api/v1/tasks \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-token" \
  -d '{
    "name": "daily-report",
    "trigger_type": "cron",
    "trigger_expr": "0 0 9 * * *",
    "command_config": {
      "program": "python",
      "args": ["generate_report.py"]
    },
    "webhook_config": {
      "url": "http://gotify:8080/message?token=xxx",
      "method": "POST",
      "body": "{\"title\":\"Report\",\"message\":\"Daily report generated\",\"priority\":5}"
    },
    "timeout_secs": 600,
    "max_retries": 2
  }'
```

### 执行历史

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/api/v1/history` | 所有历史记录（支持分页） |
| `GET` | `/api/v1/history/{id}` | 单条历史详情（含 stdout/stderr） |
| `GET` | `/api/v1/history/task/{task_id}` | 指定任务的历史记录 |

### 调度器控制

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/api/v1/scheduler/status` | 调度器状态概览 |
| `POST` | `/api/v1/scheduler/pause` | 暂停调度器 |
| `POST` | `/api/v1/scheduler/resume` | 恢复调度器 |
| `POST` | `/api/v1/scheduler/reload` | 重新加载所有任务 |

### 认证

| 方法 | 路径 | 说明 |
|---|---|---|
| `GET` | `/api/v1/auth/check` | 检查是否需要认证 |
| `POST` | `/api/v1/auth/login` | 登录（获取 Token） |

---

## 任务状态说明

| 状态 | 说明 |
|---|---|
| `Active` | 正常运行中 |
| `Paused` | 已禁用（手动或 Once 执行完成后） |
| `Completed` | 已完成（仅 Once 任务执行后） |
| `Failed` | 执行失败（所有重试耗尽后） |

---

## 技术栈

- **Rust 2024 Edition** + Tokio 异步运行时
- **Axum** — HTTP 框架
- **SQLite (rusqlite)** — 嵌入式数据库
- **Chrono** — 时间处理
- **cron** — Cron 表达式解析

## License

MIT
