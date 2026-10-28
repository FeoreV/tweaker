# План реализации задач rustweaker

Цель: выполнить доступные задачи A0–A3 и B1–B4 из `AGENT_TASKS.md`, сохранив безопасность и проверяемое восстановление; задачи C в текущем файле не определены.

- [ ] Проверить `git status`, `AGENT_START.md`, `AGENT_TASKS.md` и реальные изменения; сохранить незакоммиченное. Подтвердить A0–A3 по критериям и исходным отчётам, а не по статусам документов.
- [ ] B1: проверить preflight в `src/data/store.rs` и `file_store.rs`: все writers, максимальный размер обратной записи и запрет apply при ошибке. Целевые 4 теста прошли на хосте; повторить после изменений и проверить полный crash/fault matrix.
- [ ] B1: реализовать проверяемую ротацию заполненного recovery journal и ограниченных checkpoint generations. Сначала проверить `docs/recovery-rotation-design.md`; публиковать новый checkpoint до использования нового ID, сохранять прежнее состояние и fail-closed при неоднозначности. Проверить повторные ошибки, пределы, каждый сбой публикации и два reopen.
- [ ] B2: проверить `src/engine/registry/windows.rs` и `services.rs`: сохранение pre-state до записи, восстановление, ограничения HKCR/HKCU/HKCC, WoW64 и конфликты. Mock/fault тесты должны доказать отсутствие записи при ошибке сохранения и конфликте; реальную мутацию на хосте не запускать.
- [ ] B3: собрать `npm --prefix ui run build`, проверить отключённые действия и bridge; Sciter и Win7 проверять только при наличии явно предоставленного runtime и VM, не выдавать browser preview за Sciter.
- [ ] B4 после A1/B1: проверить `python scripts/verify-catalog-fixture.py`, схему и mock apply/undo для fixture; расширять только с проверенным источником и OS-ограничениями. Не считать `Verified` в JSON доверенным разрешением и не включать executor до закрытия safety gates.
- [ ] Обновить `AGENT_TASKS.md` и профильные документы по фактическим результатам; отдельно записать нерешённые namespace/external-writer, power-loss, Sciter licensing и Win7 VM условия. Независимую работу продолжать при внешнем блокере.
- [ ] Последняя проверка: `cargo check --locked`, `cargo test --locked -- --test-threads=1`, `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, то же для `experimental-ui`, frontend build и обе nightly Win7 сборки с аудитом импортов. Отдельно указать host, cross-build и VM evidence; не объявлять готовность без полного подтверждения.

Не изменять `winutil-26.08.19/`; сохранять Win7 x86/x64, RAII handles, отсутствие WMI и обязательное сохранение pre-state перед системной мутацией. Если план задач изменится, сверить его с текущим `AGENT_TASKS.md`, не выдумывать задачи C.

## Проверка продолжения — 2026-10-28

- Сохранены существующие правки. Проверка выполнялась в отдельном `CARGO_TARGET_DIR=target/luna-implementation`, чтобы не смешать артефакты разных Rust компиляторов. Host MSVC stable 1.97.1: `cargo check --locked`, `cargo test --locked --lib -- --test-threads=1` (138 passed), `cargo test --locked --features experimental-ui --lib -- --test-threads=1` (140 passed), `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings` — успешно. Полные тесты включали preflight, fault injection, recovery retry и subprocess crash probes; это не power-loss гарантия.
- `python scripts/verify-catalog-fixture.py` успешно: fixture подтверждает 3 переписанные операции; поддержка ОС остается непроверенной. `npm --prefix ui run build` прошел; сборка не подключает UI к backend и не подтверждает Sciter/Win7 запуск.
- Win7 cross-build пока не подтвержден в этой сессии. `rustup run nightly cargo` без явного `RUSTC` вызвал stable/nightly compiler mismatch; повтор с `RUSTC`/`RUSTDOC` на nightly 1.101.0 16d1ffac1 дошел до build-std, но `core/std` завершился ошибками несовместимых/отсутствующих Windows API symbols. Поэтому не маркировать Win7 сборки как проверенные на этом прогоне; использовать ранее зафиксированный CI-pin запуск/артефакты либо отдельно выяснить конфигурацию toolchain. VM и import audit в рамках этого прогона не запускались.
- Ротация snapshot journal в фактическом `file_store.rs` создает новый ID и следующее checkpoint generation. Она не компактизирует checkpoint history; `MAX_CHECKPOINT_GENERATIONS = MAX_TRANSACTION_RECORDS + 1` остается конечным. Повторные неудачные восстановления могут исчерпать его. Без согласованного указателя epoch, правил публикации, миграции/downgrade и удержания старого состояния компактизацию не добавлять; fail-closed и блокировка production actions остаются обязательными.
- Текущие тесты не закрывают отдельно: безопасную ротацию после исчерпания recovery journal, namespace replacement/external writers, power-loss, Win7 VM/SCM и Sciter runtime/licensing. Production UI/CLI executor отсутствует и должен оставаться выключенным.
- Дополнительно полные команды `cargo test --locked -- --test-threads=1` (138 library + 2 CLI) и `cargo test --locked --features experimental-ui -- --test-threads=1` (140 library + 2 CLI) прошли; `cargo clippy --locked --all-targets --features experimental-ui -- -D warnings` также завершился успешно. Feature Clippy показывает шесть предупреждений из vendored `sciter-rs`, но lint gate приложения проходит.

## Продолжение — 2026-10-28: совместимость журнала и отчёт служб

- Исторические записи служб формата v1 снова читаются с прежними структурными ограничениями; новые записи и прямой `sync` требуют строгой проверки. Неподдерживаемая старая запись блокируется до запроса или восстановления службы. Это не обеспечивает восстановление неподдерживаемых значений и не препятствует запуску старого бинаря.
- Добавлен read-only CLI `--services-report` для 1–32 имён: результаты состояния и конфигурации собираются независимо. На текущем Windows проверены EventLog и отсутствующее имя; системные изменения не выполнялись.
- На rustup stable MSVC 1.97.1 в `target/host-validation-20261028` прошли `cargo check --locked`, `cargo test --locked -- --test-threads=1` (144 библиотечных и 5 CLI тестов), `cargo fmt --check` и `cargo clippy --locked --all-targets -- -D warnings`. Сборка UI и `python scripts/verify-catalog-fixture.py` прошли. Win7 cross-build, VM, power-loss и feature matrix для этих изменений не проверены.
- Два ограничения ротации сохраняются: заполнение журнала снимков повторными ошибками и исчерпание поколений checkpoint способны остановить восстановление. Нужен проверенный протокол epoch/rotation; системные действия остаются отключёнными. Полный порт ещё не реализован.

## Продолжение — 2026-10-28: ёмкость журнала и каталог приложений

- При типизированном отказе preflight из-за ёмкости, до записи снимка и изменения ресурса, обе recovery-сессии публикуют новый snapshot ID с неизменным прогрессом и требуют отдельного повторного вызова. Сбой ротации блокирует текущую сессию до повторного открытия. Это не устраняет конечный предел checkpoint generations; count limit и вся crash/fault matrix новой ветви ещё не покрыты, power-loss и Win7 VM не проверены. Действия приложения остаются отключены.
- Preview приложений содержит 232 ID и выбранные поля из `winutil-26.08.19/config/applications.json`; `scripts/verify-application-preview.py` сверяет SHA-256 и точное совпадение полей, включая наличие choco. Служебный `_comment` исходника в проекцию не входит. Установка и переходы по URL не реализованы; исходная лицензия в `THIRD_PARTY/WINUTIL-LICENSE.txt`.
- Итоговый stable MSVC 1.97.1 `cargo check --locked`, полный последовательный `cargo test --locked` (148 библиотечных и 5 CLI), `cargo fmt --check` и strict all-target Clippy прошли. `npm --prefix ui run build` и проверка каталога прошли. Nightly MSVC 1.101.0 revision 16d1ffac1 собрал обе Win7 цели с `experimental-ui`, выбранная проверка импортов, CRT и subsystem 6.01 прошла. Это не полный аудит импортов или выполнение на Windows 7.
- Во время проверки инструментов агент вызвал rustup для dated alias; rustup сообщил о загрузке компонентов. Для сборок использовался уже установленный nightly MSVC; запроса на установку новых инструментов не было.

## Продолжение — 2026-10-28: данные Rust и граница журнала

- `LockedJournalStorage::sync` повторно проверяет bytes staged записи и committed prefix с допустимым incomplete tail до публикации marker. Целевые тесты проверяют прямой вызов без preflight, ограничения, подменённую запись и успешный путь. Внешняя подмена после проверки и конечная ёмкость checkpoint generations остаются открытыми; реальные системные действия не включены.
- В `config/catalog.registry-review.v1.json` внесены apply-декларации 14 целых registry-only твиков, 28 DWORD операций. `scripts/verify-registry-review.py` сверяет закреплённый SHA-256, состав, порядок и операции. Это не undo и не доказательство поддержки OS; Registry64 — политика проекта, тогда как upstream использует вид процесса PowerShell. Набор не подключён к executor.
- Добавлен read-only Rust decoder встроенного application preview и CLI `--applications-list` / `--application-show <id>`. Список возвращает 232 записи, 7zip проверен, неизвестный ID возвращает код 2. Идентификаторы и URL — только данные; установки нет.
- На stable MSVC 1.97.1 прошли cargo check, полный serial cargo test (156 library + 6 CLI; experimental-ui 158 + 6), fmt и strict all-target Clippy в обеих конфигурациях. Оба nightly Win7 release target с experimental-ui собраны и прошли выбранный импорт/CRT/subsystem аудит; VM и полный аудит импортов не выполнялись. Два source-verifier скрипта прошли; Sciter выдаёт 6 существующих vendor warnings.

## Продолжение — 2026-10-28: планирование твиков

- Добавлен `src/engine/tweak_plan.rs`: преобразование declarative registry/service в типизированные операции и mockable последовательное применение через query → persist → apply с остановкой на первом отказе. План проверяет список до выполнения, отклоняет несовместимые hive/view и неоднозначные повторы. `PlanGate` — инъекция политики, не граница доверия и не разрешение на запись; защищённый backend, сериализация и durable storage остаются ответственностью вызывающего кода. В UI/CLI и к реальному Windows backend этот путь не подключён.
- Review fixture расширен до 35 registry-only apply записей/76 операций; pinned source verifier подтверждает 70 DWORD, 1 QWORD и 5 REG_SZ и соответствующие hive/bytes. Mock-тест требует успешное применение/обратное восстановление для 16 подходящих записей и отказ по HKCU для остальных 19. Поддержка ОС, upstream process view, user context и fixed upstream undo остаются ограничениями; это не полный порт.
- В `docs/checkpoint-chain-proposal.md` описан кандидат протокола для снятия предела поколений. Он не реализован и не доказан; downgrade, Win7/NTFS publication и power-loss требуют проверки.
- На stable MSVC 1.97.1 прошли cargo check, fmt, strict all-target Clippy и serial тесты: 160 библиотечных + 6 CLI; experimental-ui 162 + 6 и strict Clippy (6 прежних vendor warnings). Оба nightly Win7 release target с experimental-ui собраны и прошли выбранный import/CRT/subsystem аудит; это не Win7 VM или полный import audit. Verifier прошёл.

## Продолжение — 2026-10-28: план и файловая транзакция

- `TweakPlan::apply_to_file_session` передаёт validated операции через `FileTypedTransactionSession::apply` и вызываемый backend; caller отвечает за авторизацию и backend. Барьер удерживается на каждую операцию, не на пакет. После ошибки нужна диагностика, incomplete tail может не допускать recovery; метод не подключён к реальному Windows backend или UI/CLI.
- NTFS host тест с mock backend проверяет marker перед apply, частичную ошибку, reopen и обратное восстановление с отдельными snapshot marker; query/persist/KeyMissing блокируют изменение. Реестр и службы хоста не изменялись.
- Добавлен review-only `config/catalog.location-review.v1.json`: `lfsvc` и три registry операции, порядок и значения проверяются `scripts/verify-location-review.py`, включая 7 негативных случаев. Upstream service OriginalType guard в плане/backend не реализован, source Original* не является backup; поддержка OS и view не подтверждены, исполнение не подключено.
- Доработана спецификация checkpoint chain: предыдущий кандидат не блокирует старые низкоуровневые writer API; остаются side-effect-free legacy validation, atomic publication, ресурсы и физическая durability. Реализации цепочки нет.
- Stable MSVC 1.97.1: cargo check и strict all-target Clippy, serial tests 163 library + 6 CLI; experimental-ui 165 + 6 и strict Clippy прошли, fmt выполнен. Nightly x86/x64 Win7 release с experimental-ui и выбранный import/CRT/subsystem аудит прошли после production-изменения; не VM или полный import audit. Шесть прежних предупреждений Sciter сохраняются.

## Продолжение — 2026-10-28: service guard и legacy checkpoint

- `SetServiceStartTypeIfExpected` проверяет expected start type по тому же queried snapshot до persist/apply под файловым mutation barrier. Несовпадение пропускает только операцию и сообщает её индекс, дальнейшие операции выполняются; список пропусков сохраняется в ошибке. Existing unconditional формат остаётся. Review fixture Location проверяет Manual (3) → Disabled (4); источник и 11 негативных случаев подтверждены. Нет wiring на реальные действия; внешняя SCM query-to-write гонка и остальные safety gates остаются.
- Выделен `validate_legacy_checkpoint_prefix` на уже удерживаемой transaction lock: чтение возвращает последнее committed состояние и точные payload/marker bytes либо отсутствие commit, не создавая файлы. `open` использует ту же проверку и по-прежнему создаёт initial checkpoint при необходимости. Содержимое staging и snapshot journal валидатор не проверяет; получение lock может создать lock файл. Chain/migration и предел generations не решены.
- Diagnostics сообщает установленную память через GetPhysicallyInstalledSystemMemory отдельно от доступной ОС; API/overflow/zero ошибки не скрывают остальные данные. Read-only host smoke прошёл.
- Stable MSVC 1.97.1 cargo check, fmt, strict all-target Clippy и полный serial test: 176 library + 6 CLI; experimental-ui 178 + 6 и strict Clippy прошли (6 прежних warnings vendor). Nightly Win7 x86/x64 release с experimental-ui и выбранный import/CRT/subsystem audit прошли; VM/power-loss/full import audit не проводились.

## Continuation — 2026-10-28: Winsock read-only report

- Завершён и подключён `src/engine/winsock_catalog.rs`: bounded read-only enumeration x86/x64 каталога Winsock и путей providers, serialization статусов; x86 процесс явно сообщает, что 64-bit каталог недоступен. Исправлено округление allocation до целого `WSAPROTOCOL_INFOW`, checked overflow/cast и лимит 4 MiB. Добавлена существующая feature `Win32_Networking_WinSock` для `windows-sys 0.59`; crates не добавлялись.
- Добавлена read-only CLI команда `--winsock-report`. Локальный запуск на Windows вернул 14 записей для обеих архитектур. Это только текущий снимок каталога, данные provider DLL/config и атомарность снимка не гарантируются.
- Сброс Winsock/TCP-IP из upstream намеренно не портирован: текущая транзакция не умеет сохранять/восстанавливать каталог provider chain и полное состояние TCP/IP.
- Проверки: default serial tests 184 library + 7 CLI; experimental-ui serial tests 186 library + 7 CLI; strict Clippy default/experimental-ui; fmt — прошли. Win7 nightly release builds experimental-ui прошли для i686/x86_64; выбранный import/subsystem audit прошёл. Win7 VM и сброс системы не выполнялись.
- Next: перед network reset спроектировать полный transactional snapshot/restore Winsock каталога и TCP/IP состояния с fault tests; отдельно продолжить checkpoint chain только после фиксации байтового формата и downgrade barrier. Application-level mutation/UI остаются disabled.
- Дополнительный review добавил общий лимит 4 MiB на provider path bytes в пределах каждого каталога, checked multiplication/addition и fail-closed поведение; 8 targeted Winsock tests и оба строгих Clippy профиля прошли. Обе Win7 сборки и выбранные import audit повторены после этого изменения.

## Continuation — 2026-10-28: application package selection

- Rust port of the pure `Get-WinUtilSelectedPackages.ps1` selector lives in `src/engine/package_plan.rs`. It preserves selection order and raw package ID strings, deduplicates separately per manager using exact string equality, skips blank/`na` IDs, and uses Chocolatey→WinGet fallback for absent/blank/`na` Chocolatey IDs. Unknown catalog IDs fail the whole plan. Application key lookup now follows upstream hashtable case-insensitivity and catalog decode rejects ASCII case-colliding IDs; both upstream and preview catalogs have 232 ASCII IDs with no case collisions.
- `--plan-install <choco|winget> <catalog-id>...` is a data-only CLI: JSON explicitly marks `plan_kind=source_package_selection_only` and `executable=false`. It does not invoke an installer or emit process arguments. Source `msstore:` prefixes and Chocolatey `git;github-desktop` remain raw IDs requiring separate source-specific normalization before any future installer.
- Exact source SHA-256 for `Get-WinUtilSelectedPackages.ps1`: `77E926774513B7798BC16681DFA5FA8E927050B54A60A427DF9A767F749DE766`. The application catalog verifier passed for all 232 entries.
- Stable MSVC 1.97.1 full serial tests passed: 194 library + 8 CLI default; 196 library + 8 CLI all-features. The package planner tests cover the five upstream entries, fallback/skip/dedupe/order, mixed-case ID lookup, unknown IDs, and source IDs with semicolons. Strict all-target/all-features Clippy and fmt passed (six existing Sciter vendor warnings). `--plan-install` and mixed-case `--application-show 7ZIP` were run on host and returned expected data.
- Nightly MSVC 1.101.0 release builds with `experimental-ui` passed for both Win7 targets; selected import/subsystem audits passed. This does not establish a Win7 VM run or actual package-manager behavior.
- Next: port the upstream `msstore:` WinGet source normalization as a typed, still non-executing conversion and model the exact Chocolatey argument boundary. Keep installer calls disabled until process invocation is constrained and package filesystem/registry effects can be recovered. Continue B1 checkpoint chain as a separate recovery gate.

### Follow-up — WinGet source resolution

- Следующий этап upstream helper `Install-WinUtilProgramWinget.ps1` теперь реализован как data-only `WingetTarget`: case-insensitive `msstore:` source selection, ровно 8-byte prefix removal, сохранение сырого ID. Пустой suffix отклоняется fail-closed (в upstream такой случай дошёл бы до запуска с пустым ID). Chocolatey `git;github-desktop` намеренно остаётся сырым значением.
- Installer helper source SHA-256: `CB3C857CF35160E163E16B0585EC7A4C9AC50995C97FE7E07894534D2D7DF936`.
- После изменения: 11/11 package-plan tests, полный stable MSVC 1.97.1 serial tests (197 library + 8 CLI default; 199 library + 8 CLI all-features), strict all-target all-features Clippy, fmt, реальный `--plan-install winget chatgpt` JSON smoke с `executable=false`, обе nightly Win7 release builds и выбранные import/subsystem audits прошли. Шесть существующих Sciter vendor warnings остаются.
- Дальше: точно моделировать исходную границу Chocolatey argument-list без запуска процесса, затем перейти к отдельному transactional installer design. Этот порт пока не выполняет установки; UI остаётся preview-only.

### Follow-up — typed Chocolatey install intent

- Upstream install flow batches the deduplicated Chocolatey IDs into one helper call and the helper inserts `-y`. Rust now exposes `ChocolateyRequest { action: Install, package_ids, assume_yes }` only when the package list is nonempty. IDs retain order, exact deduplication and semicolons.
- No command string or argv is generated. The PowerShell helper interpolates `string[]` into one `Start-Process -ArgumentList` string under ambient `$OFS`; `$OFS` can vary, and process parsing/quoting cannot be inferred as a safe Rust argv contract. The request is structured intent only. Microsoft documents that Start-Process joins argument arrays into one string and requires explicit escaping for spaces/quotes: https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.management/start-process?view=powershell-5.1
- `Install-WinUtilProgramChoco.ps1` SHA-256: `0E4D3CC34FB85CC6B3A23CCB24A0186DB320BB6F8A771569F86269BFE59ACF8B`. No PowerShell/Chocolatey execution occurred.
- After integration: 12 package-plan tests; full stable serial suites 198 library + 8 CLI default and 200 + 8 all-features; fmt and strict all-target/all-features Clippy passed. Win7 nightly release builds and selected import/subsystem audits passed for x86/x64. Win7 VM and actual package invocation remain unverified.
- Next: design the actual installation transaction, including trusted executable resolution, direct argument encoding, package manager changes, downloaded/install files, registry and services, recovery and failure outcomes. Do not turn `ChocolateyRequest` or `WingetTarget` into process launches until that contract and platform tests exist.

## Continuation — 2026-10-28: uninstall package planning

`src/engine/package_plan.rs` now represents install/uninstall intent with `PackageAction`; the original install API remains a wrapper. `--plan-uninstall <choco|winget> <catalog-id>...` emits the same `executable: false` data-only selection as install planning. Chocolatey action is typed; WinGet targets retain normalized source/ID but no process arguments are produced. Upstream Edge handling is represented by a warning flag and CLI message for its `New-Item -Force` at `%SystemRoot%\SystemApps\Microsoft.MicrosoftEdge_8wekyb3d8bbwe\MicrosoftEdge.exe`; no filesystem action ran. The actual outcome of that source command is not asserted.

The upstream uninstall workflow shares the selector, runs WinGet entries individually and Chocolatey IDs in one batch. This is not yet execution parity: no package manager is launched, and progress/exit code semantics are not implemented. Source scripts were read-only.

Verification: Stable MSVC 1.97.1 isolated run passed 201 library + 8 CLI tests; all-features passed 203 library + 8 CLI tests; strict all-target all-feature Clippy, `cargo check --locked --all-features`, fmt, CLI JSON smoke and scoped diff check passed. Nightly 1.101.0 (`16d1ffac1`, installed `rust-src`) built both Win7 x86/x64 targets with `experimental-ui`; `dumpbin` verified architecture/subsystem 6.01 and selected forbidden-import/dynamic-CRT absence. Six existing Sciter vendor warnings remain. Win7 VM, full import-table review and live package actions were not run.

Next: design a tested recovery boundary for package-manager side effects before enabling process execution. Preserve disabled UI actions and the source read-only constraint.

### Package execution boundary review

Architecture review confirms package-manager execution/bootstrap is not currently supportable under the existing transaction boundary: external processes can change unknown files, registry/service state, manager caches and remote state without a per-effect pre-write persistence seam. Installation state is not an exact undo snapshot. Keep all real package execution disabled. A future supported subset must provide a bounded effect manifest, exact snapshots, per-effect persistence/interception, conflict/readback checks, crash/partial failure handling, reverse recovery and fail-closed behavior for dependencies or network effects. The current planner remains data-only. No package tool was launched.

## Continuation — 2026-10-28: global Upgrade All intent

`Invoke-WPFInstallUpgrade.ps1` is represented by a separate `UpgradeAllPlan`, not by the selected-app action enum: Chocolatey upgrades all with `-y`; WinGet upgrades all with `--include-unknown`, silent mode and both agreement flags. The plan also records manager-bootstrap intent and WinGet's `powershell -NoExit` wrapper. `--plan-upgrade-all <choco|winget>` emits this typed JSON intent with `executable:false`; no manager detection, bootstrap, shell, network, or system write occurs. The upstream PowerShell file remains read-only and was not run.

Verification: Stable MSVC 1.97.1 full tests passed (203 library + 9 CLI default; 205 + 9 all-features), targeted package planner tests (17), strict all-target/all-feature Clippy, all-feature check, fmt, both manager CLI smoke checks and scoped diff check passed. Nightly 1.101.0 (`16d1ffac1`) built both feature-enabled Win7 targets; selected PE architecture/subsystem/import checks passed. The six existing Sciter vendor warnings remain; no Win7 VM or package operation was performed. Package manager and bootstrap execution remain blocked by unbounded side effects and missing transaction/recovery coverage.

## Continuation — 2026-10-28: declarative desired-state comparison

Added a pure comparator for typed catalog operations, with registry/service reads injected by the caller. It reports match/mismatch/missing/query-error separately from the exact observed state. This is only a reusable comparison primitive: it does not implement `Invoke-WinUtilCurrentSystem` checkbox semantics, does not infer that an operation was applied, is not connected to a production catalog, and performs no Windows I/O. The existing schema has no source-proven toggle/default/OriginalValue semantics; package detection upstream shells out to managers and was not ported here.

Verification: 5 focused tests passed; full stable MSVC suites passed (208 library + 9 CLI default; 210 + 9 all-features), strict Clippy, all-feature check, fmt, both nightly Win7 feature builds and selected PE audits. No Win7 VM/live query. Next, continue the required A1 per-entry source and compatibility audit; only then add catalog-aware read-only query behavior.
### A1 source audit — `WPFToggleDetailedBSoD` (2026-10-28)

- Added `docs/detailed-bsod-source-audit.md` for this single exact upstream toggle. The frozen catalog declares two `HKLM` CrashControl DWORDs, while the draft inventory lists one. The handler applies static `Value` / `OriginalValue` sequentially; it does not persist observed pre-state, and absent-key creation plus caught write errors allow non-exact undo and possible partial state.
- Toggle-status behavior includes a cache and substitutes configured defaults for missing values. Existing Pester coverage uses generic mocked data and does not establish this entry's Windows behavior or support matrix.
- Independent source/hash and diff checks passed. No upstream files, scripts, or Pester tests were modified/run. Do not treat the inventory's compatibility or undo labels as verified.
- Next A1 candidate: `WPFToggleNumLock`, to audit its two registry hives and context limits. This entry audit does not complete A1 or authorize production execution.

### A1 source audit — `WPFToggleNumLock` (2026-10-28)

- Added `docs/numlock-source-audit.md`. The upstream toggle declares two ordered String writes: literal `HKU:\.Default\Control Panel\Keyboard` and context-dependent `HKCU:\Control Panel\Keyboard`. The draft inventory lists only the former.
- Source does not establish the user/session identity or Windows effect of those paths. The fixed `OriginalValue` values do not capture pre-state, and sequential caught writes provide no all-or-nothing recovery. A fixture rejects its HKCU operation at the mock plan gate; this is not production behavior evidence.
- Independent source and hash review and scoped whitespace check passed. No PowerShell/Pester was run; frozen upstream remains untouched. A1 remains open; keep context-sensitive operations unavailable absent bound user identity and tested recovery.

### A1 source audit — `WPFTweaksUTC` (2026-10-28)

- Added `docs/utc-source-audit.md`. Upstream declares one HKLM QWord (`RealTimeIsUniversal`, apply `1`, fixed undo `0`) and no entry-level Toggle type. The draft inventory instead says Toggle/DWORD and `0 / DeleteValue`.
- Review fixture support remains `unverified`; its QWORD-shaped value and mock apply/recovery test do not establish Windows support or RTC semantics. Fixed undo cannot recover absent state, arbitrary type/data, or key absence.
- Independent source/hash review and scoped whitespace check passed. No PowerShell/Pester was run; upstream remains untouched. Continue A1 item-by-item; do not promote this fixture to verified.

### A1 source audit — `WPFTweaksLocation` (2026-10-28)

- Added `docs/location-source-audit.md`. Upstream declares conditional `lfsvc` startup handling followed by three registry writes; the draft inventory reports only one registry value. Service and registry setters independently catch/log errors, so the mixed batch has no all-or-nothing guarantee or persisted pre-state.
- Static `OriginalType`/`OriginalValue` values are declarations, not captured snapshots. The review fixture is `unverified`; conditional plan tests are mock evidence and do not establish Windows/SCM behavior.
- Independent source/hash review and scoped whitespace check passed. No upstream scripts/tests ran; frozen tree remains untouched. Continue A1 item-by-item and do not activate this fixture.

## Windows read-only desired-state observer — 2026-10-28

- `src/engine/tweak_state.rs` now connects the pure comparator to caller-supplied operations and existing read-only registry/service queries. It retains exact registry type/bytes and distinguishes registry vs service errors; missing service maps to `None`. A Windows smoke test queries HKLM CurrentVersion `ProductName` and EventLog configuration without writes.
- The first compile exposed an unnecessary `Clone` bound; the comparator now represents query failure as a status while retaining the original typed error in the observation. Targeted and full tests verify this.
- Stable MSVC 1.97.1: default tests 209 library + 9 CLI, all-features 211 + 9, strict all-target/all-features Clippy, all-feature check and fmt passed. Nightly 1.101.0 (`16d1ffac1`, `rust-src`) built Win7 x86/x64 release; selected import/dynamic-CRT/subsystem audits passed. No Win7 VM or system mutation. Six existing Sciter vendor warnings remain.
- This does not load a catalog, establish provenance/support or upstream checkbox parity, nor expose a CLI/UI caller. Next: design a provenance-bound, read-only status surface and asynchronous minimal UI bridge; keep mutation controls disabled until durability and recovery release gates close.

## Minimal application selection preview — 2026-10-28

- `ui/src/index.tsx` and `ui/src/styles.css` now provide checkboxes for bundled applications, a selection count, a summary of names and raw WinGet/Chocolatey IDs, and clear-selection. Selection is keyed by catalog ID and persists while search/category filters change. Empty-state guidance and keyboard focus remain present.
- Install/uninstall/upgrade buttons remain disabled. This is a local preview selection, not a manager-resolved executable plan; no Rust bridge or installed-state claim was added.
- `npm --prefix ui run build` passed. Stable MSVC 1.97.1 all-features tests (211 library + 9 CLI), all-features check, strict all-target/all-features Clippy, and fmt passed after asset generation. Experimental-ui Win7 x86/x64 nightly release builds passed; selected imports and PE machine/subsystem 6.01 audits passed. Browser interaction could not be verified because local `file:` navigation was denied and alternate browser workarounds are prohibited. No system mutation; Win7 VM and Sciter runtime checks remain unavailable.

## WinGet list text matcher — 2026-10-28

- Added bounded text-only matching for caller-supplied WinGet list output in `src/engine/package_state.rs`; `--match-winget-list` exposes it through bounded stdin in `src/main.rs`. It does not invoke WinGet and labels output as non-executable text candidates.
- Matching mirrors the upstream final-semicolon-segment parsing, case-insensitive `msstore:` removal before trim, blank/`na` filtering, and literal ID boundaries requiring at least two Unicode whitespace characters around the ID (or line end). A one-pass index keeps parsing linear in the 1 MiB input bound. Only ASCII IDs are matched case-insensitively; non-ASCII catalog IDs are skipped because Rust ASCII folding is not equivalent to .NET Unicode regex folding.
- Stable MSVC 1.97.1 all-features tests passed (227 library + 12 CLI); all-features check, strict all-target/all-features Clippy and fmt passed. CLI smoke returned Git and ChatGPT candidates for the supplied sample with `executable:false`. Nightly 1.101.0 experimental-ui Win7 x86/x64 release builds and selected import/dynamic-CRT/subsystem 6.01 audits passed. Six existing Sciter vendor warnings remain.
- No manager invocation or system mutation occurred. Win7 VM execution and full import proof remain unavailable. Continue separate read-only source ports; do not infer installed state from caller-supplied text.

## Selection import decoder — 2026-10-28

- Added pure JSON decoder/validator in `src/data/selection_import.rs`, registered by `src/data/mod.rs`. It accepts modern arrays and singleton strings, and recognizes legacy objects only when both `Install` and `WPFInstall` exist. Legacy property iteration order is preserved; only WPF selection groups are flattened, nonblank strings retained, and unknown legacy IDs skipped. Modern inputs remain strict and are fully validated before producing a proposed replacement. Duplicates/order are preserved. No app state, files, URLs, or system operations are touched.
- Caller-supplied ID catalogs are explicit per category; unavailable (`None`) is distinct from a complete catalog that rejects an unknown ID. Current partial fixtures must not be passed as complete catalogs. Matching is ASCII case-insensitive; full PowerShell Unicode case-fold parity is unverified.
- Stable MSVC 1.97.1 all-features tests passed (236 library + 12 CLI), all-features check, strict all-target/all-features Clippy, and fmt. Nine targeted decoder tests passed. Nightly 1.101.0 experimental-ui Win7 x86/x64 release builds and selected import/dynamic-CRT/subsystem 6.01 audits passed. Six existing Sciter vendor warnings remain.
- Next: inspect and port the next read-only upstream interaction; do not connect selection decoder to UI state before complete category catalogs and an explicit state contract exist.

## Package log summary formatter — 2026-10-28

- Added and registered `src/engine/package_log_summary.rs`, a pure summary formatter matching upstream field precedence, `Choco` preference/fallback, exact `na` filtering, no trimming, and stable input order/duplicates. ASCII case-insensitive comparisons are used and Unicode casing differences are documented.
- Stable MSVC 1.97.1 all-features tests passed (242 library + 12 CLI), all-features check, strict all-target/all-features Clippy, and fmt. Nightly 1.101.0 experimental-ui Win7 x86/x64 release builds and selected forbidden-import/dynamic-CRT/subsystem 6.01 audits passed. Six existing Sciter vendor warnings remain.
- It is not connected to logger or execution flows. It accepts caller-provided optional text fields and performs no package/system actions. Continue with a bounded read-only port; keep package execution unavailable pending effect/recovery gates.

## Imported application package-plan CLI — 2026-10-28

- Added `src/engine/package_import.rs` to compose bounded selection import with the preview application catalog and existing planner. `--plan-import <choco|winget> <install|uninstall>` reads JSON from stdin and returns typed plan data with `executable:false` and `catalog_coverage=preview_subset_only`.
- It maps `WPFInstall{key}` to catalog IDs, rejects selected non-app categories instead of returning a partial plan, and fails closed when a legacy key is absent from the supplied catalog because that does not prove upstream retirement. Modern unknowns fail atomically. A PowerShell key comparison found 232 keys each in the preview and frozen upstream catalog; this checks key set only, not provenance or support.
- CLI errors name unsupported keys. Imported WinGet Edge uninstall includes the upstream `New-Item -Force` warning. No process, file, UI state, or system operation is performed.
- Stable MSVC 1.97.1 all-features tests passed (250 library + 15 CLI), all-features check, strict all-target/all-features Clippy, and fmt. CLI smoke verified Choco uninstall output and WinGet Edge warning. Nightly 1.101.0 experimental-ui Win7 x86/x64 release builds and selected import/dynamic-CRT/subsystem 6.01 checks passed. Six Sciter vendor warnings remain.
- Execution is still unavailable: installer effects and rollback are not captured. Full UI state integration, provenance certification, Win7 VM, and full PE import audit remain open.
## Continuation — 2026-10-28: application selection export

`--export-app-selection` accepts application catalog IDs, validates the complete ID set before writing stdout, and emits WinUtil JSON with a scalar for one ID or an array for multiple IDs. IDs use the `WPFInstall` prefix. The package planner accepts the result through stdin. Export coverage is `preview_subset_only`, and output is marked `executable:false`. This flow does not access the clipboard, write a file, use a UI bridge, or execute system/package actions.

Verification recorded for this handoff: stable MSVC 1.97.1 all-features library tests passed (255) and CLI tests passed (17); all-features `cargo check`, strict all-target/all-features Clippy, and fmt passed. Piped `--export-app-selection 7zip` into `--plan-import winget install`; the planner returned `7zip.7zip` with `executable:false`. Nightly 1.101.0 experimental-ui Win7 x86/x64 release builds passed; selected forbidden-import/dynamic-CRT and PE subsystem 6.01 audits passed. Six existing Sciter vendor warnings remain. Win7 VM execution and full PE import auditing are unverified. This does not establish full upstream catalog provenance or execution support; preserve the preview-only boundary.

## Continuation — 2026-10-28: application catalog filter

Added pure filtering in `src/engine/application_filter.rs`, registered in `src/engine/mod.rs`. Search criteria cover content, description, ID, and full `WPFInstall{id}` tag with literal ASCII case-insensitive matching; category matching is ASCII-insensitive exact match. Blank criteria are ignored, active criteria combine with AND, and catalog order is preserved. Unicode case-fold parity is not claimed; auto-expand/collapse and UI state are out of scope.

Verification: Stable all-features tests passed (261 library + 17 CLI); locked all-features check, strict locked all-target/all-features Clippy, and fmt check passed. Nightly `experimental-ui` Win7 x86/x64 release builds and selected PE audits passed. Frozen upstream directory remained clean. This does not establish full import-table or Win7 runtime validation.

## Continuation — 2026-10-28: read-only installed registry app inventory

Added `--installed-registry-apps` over `src/engine/installed_apps.rs` and `src/engine/installed_apps_windows.rs` (registered in `src/engine/mod.rs`, exposed in `src/main.rs`). It enumerates HKLM/HKCU; uses explicit 64-bit and 32-bit views on 64-bit Windows and Native view on 32-bit Windows, with x86 WOW64 detection through `IsWow64Process`. Output fields are `DisplayName`, `DisplayVersion`, and `Publisher`. Enumeration bounds keys, values, and bytes; decodes UTF-16 `REG_SZ`/`REG_EXPAND_SZ` without expansion; reports errors and partial status explicitly; and does not deduplicate across hives/views.

This is an Add/Remove Programs registry registration inventory, not parity with upstream `Invoke-WinUtilCurrentSystem` package IDs. It does not launch package managers or match catalog entries by source/publisher.

Verification: Stable all-features suite passed (269 library + 18 CLI), all-features check, strict all-target/all-features Clippy, and fmt check. Read-only runtime smoke returned `Complete` across HKLM/HKCU views; aggregate entry counts were 86, 122, 19, and 19. Nightly `experimental-ui` Win7 x86/x64 release builds and selected PE import/subsystem audits passed. No Win7 VM or full import audit; frozen upstream was not edited.

## Continuation — 2026-10-28: limited read-only toggle status observer

Added `--toggle-status <WPFToggleDetailedBSoD> <native|64|32>` for this ID only; this is not a general `Get-WinUtilToggleStatus` port. It reads `HKLM\\SYSTEM\\CurrentControlSet\\Control\\CrashControl` values `DisplayParameters` and `DisableEmoticon` as `REG_DWORD`, with desired value `1`, source `OriginalValue` `0`, and `DefaultState` false. Both values equal to 1 report checked; missing key/value uses the source default and reports unchecked; a differing DWORD reports unchecked. Query errors, unsupported types, and unsupported lengths report `unknown`. Registry view is explicitly caller-selected. No writes, cache, or UI integration are involved.

`unknown` on errors is an intentional safer divergence from PowerShell's nonterminating errors; non-DWORD values are not coercively compared. No Windows effect or support claim is established.

Verification: Stable all-features tests passed (273 library + 19 CLI); locked all-features check, strict all-target/all-features Clippy, and fmt passed. Read-only Registry64 smoke reported checked for the two observed values (raw bytes omitted). Nightly `experimental-ui` Win7 x86/x64 release builds and selected PE audits passed. No Win7 VM or full import audit; frozen source remained unchanged.
