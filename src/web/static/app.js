let token = localStorage.getItem('auth_token') || '';
let currentPage = 'dashboard';

const API = '/api/v1';

function headers() {
    const h = { 'Content-Type': 'application/json' };
    if (token) h['Authorization'] = 'Bearer ' + token;
    return h;
}

async function api(path, opts = {}) {
    const resp = await fetch(API + path, { headers: headers(), ...opts });
    if (resp.status === 401) {
        showAuth();
        throw new Error('Unauthorized');
    }
    if (!resp.ok && resp.status !== 204) {
        const text = await resp.text();
        throw new Error(text || resp.statusText);
    }
    if (resp.status === 204) return null;
    return resp.json();
}

function showAuth() {
    document.getElementById('auth-screen').style.display = 'flex';
    document.getElementById('app').style.display = 'none';
}

function hideAuth() {
    document.getElementById('auth-screen').style.display = 'none';
    document.getElementById('app').style.display = 'block';
}

function formatTime(iso) {
    if (!iso) return '-';
    const d = new Date(iso);
    return d.toLocaleString();
}

function relTime(iso) {
    if (!iso) return '-';
    const now = Date.now();
    const t = new Date(iso).getTime();
    const diff = t - now;
    const absDiff = Math.abs(diff);
    if (absDiff < 60000) return diff > 0 ? 'in <1m' : '<1m ago';
    if (absDiff < 3600000) {
        const m = Math.floor(absDiff / 60000);
        return diff > 0 ? `in ${m}m` : `${m}m ago`;
    }
    if (absDiff < 86400000) {
        const h = Math.floor(absDiff / 3600000);
        return diff > 0 ? `in ${h}h` : `${h}h ago`;
    }
    const d = Math.floor(absDiff / 86400000);
    return diff > 0 ? `in ${d}d` : `${d}d ago`;
}

function badge(cls, text) {
    return `<span class="badge badge-${cls}">${text}</span>`;
}

let tasksAutoRefreshTimer = null;
let historyAutoRefreshTimer = null;

function navigate(page) {
    currentPage = page;
    document.querySelectorAll('.page').forEach(p => p.style.display = 'none');
    document.querySelectorAll('.nav-links a').forEach(a => a.classList.remove('active'));
    document.getElementById('page-' + page).style.display = 'block';
    document.querySelector(`[data-page="${page}"]`).classList.add('active');
    clearInterval(tasksAutoRefreshTimer);
    clearInterval(historyAutoRefreshTimer);
    tasksAutoRefreshTimer = null;
    historyAutoRefreshTimer = null;
    if (page === 'dashboard') loadDashboard();
    else if (page === 'tasks') {
        loadTasks();
        tasksAutoRefreshTimer = setInterval(() => loadTasks(), 5000);
    }
    else if (page === 'history') {
        loadHistory();
        historyAutoRefreshTimer = setInterval(() => loadHistory(), 5000);
    }
}

async function loadDashboard() {
    try {
        const data = await api('/scheduler/status');
        document.getElementById('stats-cards').innerHTML = `
            <div class="stat-card"><div class="label">Total Tasks</div><div class="value">${data.total_tasks}</div></div>
            <div class="stat-card"><div class="label">Active</div><div class="value active">${data.active_tasks}</div></div>
            <div class="stat-card"><div class="label">Paused</div><div class="value paused">${data.paused_tasks}</div></div>
            <div class="stat-card"><div class="label">Failed</div><div class="value failed">${data.failed_tasks}</div></div>
            <div class="stat-card"><div class="label">Runs Today</div><div class="value">${data.runs_today}</div></div>
        `;
    } catch (e) {
        console.error(e);
        document.getElementById('stats-cards').innerHTML = `<div class="empty-state">Failed to load dashboard: ${esc(e.message)}</div>`;
    }
}

let tasksPage = 1;
let tasksPerPage = 20;

async function loadTasks(page) {
    if (page) tasksPage = page;
    const search = document.getElementById('task-search').value;
    const status = document.getElementById('task-status-filter').value;
    const trigger = document.getElementById('task-trigger-filter').value;
    let qs = `?page=${tasksPage}&per_page=${tasksPerPage}`;
    if (search) qs += `&search=${encodeURIComponent(search)}`;
    if (status) qs += `&status=${status}`;
    if (trigger) qs += `&trigger_type=${trigger}`;
    try {
        const data = await api('/tasks' + qs);
        const rows = data.items.map(t => {
            const lastResult = t.last_run_status
                ? badge(t.last_run_status === 'success' ? 'success' : 'failed', t.last_run_status)
                : '-';
            return `
            <tr>
                <td><a href="#" onclick="viewTask('${t.id}');return false" style="color:var(--primary)">${esc(t.name)}</a></td>
                <td>${badge(t.status, t.status)}</td>
                <td>${t.trigger_type}</td>
                <td><code>${esc(t.trigger_expr)}</code></td>
                <td>${t.enabled ? badge('active','ON') : badge('paused','OFF')}</td>
                <td>${relTime(t.last_run_at)}</td>
                <td>${lastResult}</td>
                <td>${relTime(t.next_run_at)}</td>
                <td class="actions-cell">
                    <button class="btn btn-sm btn-outline" onclick="triggerTask('${t.id}')">Run</button>
                    <button class="btn btn-sm btn-outline" onclick="toggleTask('${t.id}',${!t.enabled})">${t.enabled ? 'Disable' : 'Enable'}</button>
                    <button class="btn btn-sm btn-outline" onclick="editTask('${t.id}')">Edit</button>
                    <button class="btn btn-sm btn-danger" onclick="deleteTask('${t.id}','${esc(t.name)}')">Del</button>
                </td>
            </tr>
        `}).join('');
        const totalPages = Math.ceil(data.total / data.per_page);
        let pagination = '<div class="pagination">';
        for (let i = 1; i <= totalPages && i <= 20; i++) {
            pagination += `<button class="btn btn-sm ${i === tasksPage ? 'btn-primary' : 'btn-outline'}" onclick="loadTasks(${i})">${i}</button>`;
        }
        pagination += '</div>';
        document.getElementById('tasks-table-container').innerHTML = data.items.length ? `
            <table>
                <thead><tr><th>Name</th><th>Status</th><th>Trigger</th><th>Expression</th><th>Enabled</th><th>Last Run</th><th>Last Result</th><th>Next Run</th><th>Actions</th></tr></thead>
                <tbody>${rows}</tbody>
            </table>
            ${pagination}
        ` : '<div class="empty-state">No tasks found</div>';
    } catch (e) {
        console.error(e);
        document.getElementById('tasks-table-container').innerHTML = `<div class="empty-state">Failed to load tasks: ${esc(e.message)}</div>`;
    }
}

let historyPage = 1;
let historyPerPage = 20;

async function loadHistory(page) {
    if (page) historyPage = page;
    const taskName = document.getElementById('history-task-filter').value;
    const status = document.getElementById('history-status-filter').value;
    let qs = `?page=${historyPage}&per_page=${historyPerPage}`;
    if (taskName) qs += `&task_name=${encodeURIComponent(taskName)}`;
    if (status) qs += `&status=${status}`;
    try {
        const data = await api('/history' + qs);
        const rows = data.items.map(h => `
            <tr onclick="viewHistory('${h.id}')" style="cursor:pointer">
                <td>${esc(h.task_name)}</td>
                <td>${badge(h.status, h.status)}</td>
                <td>${formatTime(h.started_at)}</td>
                <td>${formatTime(h.finished_at)}</td>
                <td>${h.exit_code ?? '-'}</td>
                <td>${h.error_msg ? esc(h.error_msg).substring(0,50) : '-'}</td>
            </tr>
        `).join('');
        const totalPages = Math.ceil(data.total / data.per_page);
        let pagination = '<div class="pagination">';
        for (let i = 1; i <= totalPages && i <= 20; i++) {
            pagination += `<button class="btn btn-sm ${i === historyPage ? 'btn-primary' : 'btn-outline'}" onclick="loadHistory(${i})">${i}</button>`;
        }
        pagination += '</div>';
        document.getElementById('history-table-container').innerHTML = data.items.length ? `
            <table>
                <thead><tr><th>Task Name</th><th>Status</th><th>Started</th><th>Finished</th><th>Exit Code</th><th>Error</th></tr></thead>
                <tbody>${rows}</tbody>
            </table>
            ${pagination}
        ` : '<div class="empty-state">No history records</div>';
    } catch (e) {
        console.error(e);
        document.getElementById('history-table-container').innerHTML = `<div class="empty-state">Failed to load history: ${esc(e.message)}</div>`;
    }
}

async function clearAllHistory() {
    if (!confirm('Clear all execution history? This cannot be undone.')) return;
    try {
        await api('/history', { method: 'DELETE' });
        loadHistory();
    } catch (e) {
        alert('Clear failed: ' + e.message);
    }
}

async function viewTask(id) {
    try {
        const task = await api('/tasks/' + id);
        const history = await api('/history/task/' + id + '?per_page=10');
        const hRows = history.items.map(h => `
            <tr onclick="viewHistory('${h.id}')" style="cursor:pointer">
                <td>${badge(h.status, h.status)}</td>
                <td>${formatTime(h.started_at)}</td>
                <td>${formatTime(h.finished_at)}</td>
                <td>${h.exit_code ?? '-'}</td>
            </tr>
        `).join('');
        document.getElementById('detail-title').textContent = task.name;
        document.getElementById('detail-body').innerHTML = `
            <div class="detail-section">
                <h3>Task Info</h3>
                <div class="detail-grid">
                    <div class="detail-item"><div class="label">ID</div><div class="val">${task.id}</div></div>
                    <div class="detail-item"><div class="label">Status</div><div class="val">${badge(task.status, task.status)}</div></div>
                    <div class="detail-item"><div class="label">Trigger</div><div class="val">${task.trigger_type} - <code>${esc(task.trigger_expr)}</code>${task.trigger_type === 'cron' && task.cron_tz_mode === 'local' ? ' <span class="badge badge-active">LOCAL</span>' : ''}</div></div>
                    <div class="detail-item"><div class="label">Action</div><div class="val">${task.action_type}</div></div>
                    <div class="detail-item"><div class="label">Enabled</div><div class="val">${task.enabled ? 'Yes' : 'No'}</div></div>
                    <div class="detail-item"><div class="label">Max Retries</div><div class="val">${task.max_retries}</div></div>
                    <div class="detail-item"><div class="label">Timeout</div><div class="val">${task.timeout_secs ?? '-'}s</div></div>
                    <div class="detail-item"><div class="label">Gotify Token</div><div class="val">${task.gotify_token ? '***' : '-'}</div></div>
                    <div class="detail-item"><div class="label">Created</div><div class="val">${formatTime(task.created_at)}</div></div>
                    <div class="detail-item"><div class="label">Last Run</div><div class="val">${formatTime(task.last_run_at)}</div></div>
                    <div class="detail-item"><div class="label">Last Result</div><div class="val">${task.last_run_status ? badge(task.last_run_status === 'success' ? 'success' : 'failed', task.last_run_status) : '-'}</div></div>
                    <div class="detail-item"><div class="label">Next Run</div><div class="val">${formatTime(task.next_run_at)}</div></div>
                </div>
            </div>
            <div class="detail-section">
                <h3>Action Config</h3>
                <pre class="output-block">${JSON.stringify(task.action_config, null, 2)}</pre>
            </div>
            <div class="detail-section">
                <h3>Recent Runs</h3>
                ${hRows.length ? `<table><thead><tr><th>Status</th><th>Started</th><th>Finished</th><th>Exit</th></tr></thead><tbody>${hRows}</tbody></table>` : '<div class="empty-state">No runs yet</div>'}
            </div>
        `;
        document.getElementById('detail-modal').style.display = 'flex';
    } catch (e) {
        console.error(e);
    }
}

async function viewHistory(id) {
    try {
        const h = await api('/history/' + id);
        document.getElementById('detail-title').textContent = 'Execution Detail';
        document.getElementById('detail-body').innerHTML = `
            <div class="detail-section">
                <h3>Run Info</h3>
                <div class="detail-grid">
                    <div class="detail-item"><div class="label">ID</div><div class="val">${h.id}</div></div>
                    <div class="detail-item"><div class="label">Task ID</div><div class="val">${h.task_id}</div></div>
                    <div class="detail-item"><div class="label">Status</div><div class="val">${badge(h.status, h.status)}</div></div>
                    <div class="detail-item"><div class="label">Exit Code</div><div class="val">${h.exit_code ?? '-'}</div></div>
                    <div class="detail-item"><div class="label">Started</div><div class="val">${formatTime(h.started_at)}</div></div>
                    <div class="detail-item"><div class="label">Finished</div><div class="val">${formatTime(h.finished_at)}</div></div>
                </div>
            </div>
            ${h.stdout ? `<div class="detail-section"><h3>Stdout</h3><pre class="output-block">${esc(h.stdout)}</pre></div>` : ''}
            ${h.stderr ? `<div class="detail-section"><h3>Stderr</h3><pre class="output-block">${esc(h.stderr)}</pre></div>` : ''}
            ${h.error_msg ? `<div class="detail-section"><h3>Error</h3><pre class="output-block">${esc(h.error_msg)}</pre></div>` : ''}
        `;
        document.getElementById('detail-modal').style.display = 'flex';
    } catch (e) {
        console.error(e);
    }
}

async function triggerTask(id) {
    try {
        await api('/tasks/' + id + '/trigger', { method: 'POST' });
        loadTasks();
    } catch (e) {
        alert('Trigger failed: ' + e.message);
    }
}

async function toggleTask(id, enable) {
    try {
        await api('/tasks/' + id + '/' + (enable ? 'enable' : 'disable'), { method: 'POST' });
        loadTasks();
    } catch (e) {
        alert('Toggle failed: ' + e.message);
    }
}

async function deleteTask(id, name) {
    if (!confirm(`Delete task "${name}"?`)) return;
    try {
        await api('/tasks/' + id, { method: 'DELETE' });
        loadTasks();
    } catch (e) {
        alert('Delete failed: ' + e.message);
    }
}

function openNewTaskModal() {
    document.getElementById('modal-title').textContent = 'New Task';
    document.getElementById('task-form').reset();
    document.getElementById('task-id').value = '';
    document.getElementById('task-timeout').value = '3600';
    document.getElementById('task-max-retries').value = '0';
    document.getElementById('task-cron-tz').value = 'utc';
    updateActionConfig();
    updateTriggerLabel();
    document.getElementById('task-modal').style.display = 'flex';
}

async function editTask(id) {
    try {
        const t = await api('/tasks/' + id);
        document.getElementById('modal-title').textContent = 'Edit Task';
        document.getElementById('task-id').value = t.id;
        document.getElementById('task-name').value = t.name;
        document.getElementById('task-description').value = t.description;
        document.getElementById('task-trigger-type').value = t.trigger_type;
        document.getElementById('task-cron-tz').value = t.cron_tz_mode || 'utc';
        document.getElementById('task-trigger-expr').value = t.trigger_expr;
        document.getElementById('task-action-type').value = t.action_type;
        document.getElementById('task-max-retries').value = t.max_retries;
        document.getElementById('task-timeout').value = t.timeout_secs ?? 3600;
        document.getElementById('task-gotify-token').value = t.gotify_token || '';
        updateTriggerLabel();
        updateActionConfig();
        if (t.action_type === 'command') {
            const c = t.action_config;
            document.getElementById('task-program').value = c.program || '';
            document.getElementById('task-args').value = (c.args || []).join('\n');
            document.getElementById('task-workdir').value = c.working_dir || '';
        } else if (t.action_type === 'webhook') {
            const w = t.action_config;
            document.getElementById('task-webhook-url').value = w.url || '';
            document.getElementById('task-webhook-method').value = w.method || 'GET';
            document.getElementById('task-webhook-body').value = w.body || '';
        }
        document.getElementById('task-modal').style.display = 'flex';
    } catch (e) {
        console.error(e);
    }
}

function updateTriggerLabel() {
    const tt = document.getElementById('task-trigger-type').value;
    const labels = { cron: 'Cron Expression', once: 'Run At (ISO 8601)', interval: 'Interval (seconds)' };
    const placeholders = { cron: '0 0 8 * * *', once: '2026-06-01T08:00:00Z', interval: '3600' };
    document.getElementById('trigger-expr-label').textContent = labels[tt];
    document.getElementById('task-trigger-expr').placeholder = placeholders[tt];
    const tzGroup = document.getElementById('cron-tz-group');
    const exprGroup = document.getElementById('trigger-expr-group');
    if (tt === 'cron') {
        tzGroup.style.display = '';
        exprGroup.style.gridColumn = '';
        updateCronPreview();
    } else {
        tzGroup.style.display = 'none';
        exprGroup.style.gridColumn = 'span 2';
        document.getElementById('cron-preview').style.display = 'none';
    }
}

function updateActionConfig() {
    const at = document.getElementById('task-action-type').value;
    document.getElementById('command-config').style.display = at === 'command' ? 'block' : 'none';
    document.getElementById('webhook-config').style.display = at === 'webhook' ? 'block' : 'none';
}

function parseCronField(field, min, max) {
    if (field === '*') return null;
    const vals = new Set();
    for (const part of field.split(',')) {
        const stepMatch = part.match(/^(\*|\d+-\d+)\/(\d+)$/);
        if (stepMatch) {
            const step = parseInt(stepMatch[2]);
            let s = min, e = max;
            if (stepMatch[1] !== '*') {
                const r = stepMatch[1].split('-');
                s = parseInt(r[0]); e = parseInt(r[1]);
            }
            for (let v = s; v <= e; v += step) vals.add(v);
            continue;
        }
        const rangeMatch = part.match(/^(\d+)-(\d+)$/);
        if (rangeMatch) {
            for (let v = parseInt(rangeMatch[1]); v <= parseInt(rangeMatch[2]); v++) vals.add(v);
            continue;
        }
        if (/^\d+$/.test(part)) { vals.add(parseInt(part)); continue; }
        return null;
    }
    return vals;
}

function cronFieldMatches(field, value, min, max) {
    if (!field || field === '*') return true;
    const vals = parseCronField(field, min, max);
    return vals ? vals.has(value) : false;
}

function updateCronPreview() {
    const expr = document.getElementById('task-trigger-expr').value.trim();
    const preview = document.getElementById('cron-preview');
    const tzMode = document.getElementById('task-cron-tz').value;
    if (!expr) { preview.style.display = 'none'; return; }
    const fields = expr.split(/\s+/);
    if (fields.length < 5 || fields.length > 6) { preview.style.display = 'none'; return; }
    const fSec  = fields.length === 6 ? fields[0] : '0';
    const fMin  = fields.length === 6 ? fields[1] : fields[0];
    const fHour = fields.length === 6 ? fields[2] : fields[1];
    const fDom  = fields.length === 6 ? fields[3] : fields[2];
    const fMon  = fields.length === 6 ? fields[4] : fields[3];
    const fDow  = fields.length === 6 ? fields[5] : fields[4];
    const secField  = parseCronField(fSec, 0, 59);
    const minField  = parseCronField(fMin, 0, 59);
    const hourField = parseCronField(fHour, 0, 23);
    const domField  = parseCronField(fDom, 1, 31);
    const monField  = parseCronField(fMon, 1, 12);
    const dowField  = parseCronField(fDow, 0, 6);
    if (secField === null && fSec !== '*' ||
        minField === null && fMin !== '*' ||
        hourField === null && fHour !== '*' ||
        domField === null && fDom !== '*' ||
        monField === null && fMon !== '*' ||
        dowField === null && fDow !== '*') {
        preview.style.display = 'none'; return;
    }
    const matches = (sec, min, hour, dom, mon, dow) =>
        (secField === null ? sec === 0 : secField.has(sec)) &&
        (minField === null ? true : minField.has(min)) &&
        (hourField === null ? true : hourField.has(hour)) &&
        (domField === null ? true : domField.has(dom)) &&
        (monField === null ? true : monField.has(mon)) &&
        (dowField === null ? true : dowField.has(dow));

    const useLocal = tzMode === 'local';
    const now = useLocal ? new Date() : new Date(new Date().toISOString().slice(0, -1) + '+00:00');
    const start = new Date(now.getTime());
    start.setSeconds(0, 0);
    start.setMinutes(start.getMinutes() + 1);
    const results = [];
    for (let i = 0; i < 600000 && results.length < 3; i++) {
        const d = new Date(start.getTime() + i * 60000);
        const sec = d.getUTCSeconds();
        const min = useLocal ? d.getMinutes() : d.getUTCMinutes();
        const hour = useLocal ? d.getHours() : d.getUTCHours();
        const dom = useLocal ? d.getDate() : d.getUTCDate();
        const mon = (useLocal ? d.getMonth() : d.getUTCMonth()) + 1;
        const dow = useLocal ? d.getDay() : d.getUTCDay();
        if (matches(sec, min, hour, dom, mon, dow)) results.push(d);
    }
    if (!results.length) { preview.style.display = 'none'; return; }
    const tz = tzMode === 'local' ? 'Local' : 'UTC';
    const lines = results.map(d => {
        const s = useLocal ? d.toLocaleString() : d.toISOString().replace('T', ' ').replace(/\.\d+Z$/, ' UTC');
        return `<div>${s}</div>`;
    });
    preview.innerHTML = `<div class="cron-preview-title">Next runs (${tz}):</div>${lines.join('')}`;
    preview.style.display = 'block';
}

async function saveTask(e) {
    e.preventDefault();
    const id = document.getElementById('task-id').value;
    const tt = document.getElementById('task-trigger-type').value;
    const at = document.getElementById('task-action-type').value;
    let actionConfig;
    if (at === 'command') {
        const args = document.getElementById('task-args').value.split('\n').filter(s => s.trim());
        const workdir = document.getElementById('task-workdir').value.trim();
        actionConfig = {
            program: document.getElementById('task-program').value,
            args,
            env: {},
        };
        if (workdir) actionConfig.working_dir = workdir;
    } else {
        const body = document.getElementById('task-webhook-body').value.trim();
        actionConfig = {
            url: document.getElementById('task-webhook-url').value,
            method: document.getElementById('task-webhook-method').value,
        };
        if (body) actionConfig.body = body;
    }

    const gotifyToken = document.getElementById('task-gotify-token').value.trim();
    const payload = {
        name: document.getElementById('task-name').value,
        description: document.getElementById('task-description').value,
        trigger_type: tt,
        trigger_expr: document.getElementById('task-trigger-expr').value,
        action_type: at,
        action_config: actionConfig,
        max_retries: parseInt(document.getElementById('task-max-retries').value) || 0,
        timeout_secs: parseInt(document.getElementById('task-timeout').value) || 3600,
    };
    if (tt === 'cron') {
        payload.cron_tz_mode = document.getElementById('task-cron-tz').value;
    }
    if (gotifyToken) payload.gotify_token = gotifyToken;

    try {
        if (id) {
            await api('/tasks/' + id, { method: 'PUT', body: JSON.stringify(payload) });
        } else {
            await api('/tasks', { method: 'POST', body: JSON.stringify(payload) });
        }
        document.getElementById('task-modal').style.display = 'none';
        loadTasks();
    } catch (e) {
        alert('Save failed: ' + e.message);
    }
}

function esc(s) {
    if (!s) return '';
    const d = document.createElement('div');
    d.textContent = s;
    return d.innerHTML;
}

document.addEventListener('DOMContentLoaded', async () => {
    try {
        const check = await fetch(API + '/auth/check');
        const data = await check.json();
        if (data.auth_required) {
            if (!token) {
                showAuth();
            } else {
                hideAuth();
                document.getElementById('btn-logout').style.display = 'inline-block';
            }
        } else {
            hideAuth();
        }
    } catch {
        hideAuth();
    }

    document.querySelectorAll('.nav-links a').forEach(a => {
        a.addEventListener('click', e => {
            e.preventDefault();
            navigate(a.dataset.page);
        });
    });

    async function doLogin() {
        const t = document.getElementById('auth-token-input').value.trim();
        if (!t) return;
        document.getElementById('auth-error').style.display = 'none';
        try {
            const resp = await fetch(API + '/auth/login', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ token: t }),
            });
            if (resp.ok) {
                token = t;
                localStorage.setItem('auth_token', t);
                hideAuth();
                document.getElementById('btn-logout').style.display = 'inline-block';
                navigate('dashboard');
            } else {
                document.getElementById('auth-error').style.display = 'block';
                document.getElementById('auth-error').textContent = 'Invalid token';
            }
        } catch (e) {
            document.getElementById('auth-error').style.display = 'block';
            document.getElementById('auth-error').textContent = e.message;
        }
    }

    document.getElementById('btn-login').addEventListener('click', doLogin);
    document.getElementById('auth-token-input').addEventListener('keydown', (e) => {
        if (e.key === 'Enter') { e.preventDefault(); doLogin(); }
    });

    document.getElementById('btn-logout').addEventListener('click', () => {
        token = '';
        localStorage.removeItem('auth_token');
        showAuth();
    });

    document.getElementById('btn-new-task').addEventListener('click', openNewTaskModal);
    document.getElementById('modal-close').addEventListener('click', () => document.getElementById('task-modal').style.display = 'none');
    document.getElementById('btn-cancel').addEventListener('click', () => document.getElementById('task-modal').style.display = 'none');
    document.getElementById('detail-modal-close').addEventListener('click', () => document.getElementById('detail-modal').style.display = 'none');
    document.getElementById('task-form').addEventListener('submit', saveTask);
    document.getElementById('task-trigger-type').addEventListener('change', updateTriggerLabel);
    document.getElementById('task-action-type').addEventListener('change', updateActionConfig);
    document.getElementById('task-trigger-expr').addEventListener('input', updateCronPreview);
    document.getElementById('task-cron-tz').addEventListener('change', updateCronPreview);

    let searchTimer;
    document.getElementById('task-search').addEventListener('input', () => {
        clearTimeout(searchTimer);
        searchTimer = setTimeout(() => loadTasks(1), 300);
    });
    document.getElementById('task-status-filter').addEventListener('change', () => loadTasks(1));
    document.getElementById('task-trigger-filter').addEventListener('change', () => loadTasks(1));

    let historySearchTimer;
    document.getElementById('history-task-filter').addEventListener('input', () => {
        clearTimeout(historySearchTimer);
        historySearchTimer = setTimeout(() => loadHistory(1), 300);
    });
    document.getElementById('history-status-filter').addEventListener('change', () => loadHistory(1));

    document.querySelectorAll('.modal').forEach(m => {
        m.addEventListener('click', e => {
            if (e.target === m) m.style.display = 'none';
        });
    });

    navigate('dashboard');
});
