# adaptive-ji 1.2.0

Контекстный **adaptive just intonation** для MIDI: обычные ноты на входе,
динамически настраиваемые рациональные отношения на выходе. Доступен как
**CLAP / VST3 note-effect плагин** и как **standalone-программа для Linux**.
Это не синтезатор: для звука нужен отдельный MPE-совместимый инструмент.

```text
MIDI / нотная дорожка → adaptive-ji → MPE-синтезатор → звук
```

При изменении аккорда уже звучащие ноты перестраиваются без повторной атаки.
Scala задаёт базовые высоты, а не заменяет адаптивную настройку фиксированным строем.

## Возможности

- Общий движок для standalone, CLAP и VST3; до 15 независимых голосов.
- Произвольный prime limit: простой `p` разрешает множители ≤ `p`.
  Complexity-bounded генерация кандидатов и детерминированный beam search.
- Попарная сложность отношений, спектральная sensory roughness,
  floating anchor и штраф за движение от текущей интерполированной высоты.
- Sustain-aware голоса, отдельные экземпляры повторных нот, плавный glide.
- Экспериментальная память недавних рациональных отношений между pitch classes.
- Scala `.scl` с необязательным `.kbm`, включая неоктавные периоды.
- Спектры harmonic, sine, saw, square, soft, bright и пользовательский TOML.
- Редактор с таблицей звучащих нот, отношениями, текущими/целевыми высотами и Panic.

## Быстрый старт: плагин

Проверяемая платформа сборки — Linux. Нужны Rust с поддержкой edition 2024,
компилятор C/C++, `pkg-config` и development-библиотеки X11/OpenGL для редактора.
Под Wayland редактор использует XWayland.

```sh
cargo run -p xtask -- bundle adaptive-ji-plugin --release
```

Результаты сборки:

| Формат | Файл / bundle |
| --- | --- |
| CLAP | `target/bundled/adaptive-ji-plugin.clap` |
| VST3 | `target/bundled/adaptive-ji-plugin.vst3/` |

Скопируйте нужный bundle в каталог плагинов, который сканирует ваш хост
(обычно `~/.clap/` или `~/.vst3/`), и выполните повторное сканирование.
Для VST3 копируется **весь каталог `.vst3`**, а не только библиотека внутри него.
Перед заменой загруженной версии закройте хост.

Хост должен поддерживать MIDI/note-effect routing: направьте выход событий
adaptive-ji на вход инструмента. Плагин не имеет аудиовходов/аудиовыходов.
Недостаточно вставить его в цепь, которая передаёт только аудио.

В принимающем инструменте:

1. Включите MPE с member channels **2..16** (канал 1 — master).
2. Установите pitch-bend range **±2 полутона**, либо то же значение,
   что выбрано в adaptive-ji. RPN также отправляется, но поддерживается не всеми инструментами.
3. Уменьшите дополнительный bend smoothing, если используете glide адаптера,
   чтобы не получить двойное сглаживание.

### Два режима атаки

| Режим | Поведение |
| --- | --- |
| **Сразу выдать ноту** / `immediate` (по умолчанию) | NoteOn сразу на базовой высоте Scala/12-TET; worker подбирает JI, затем голоса плавно переходят к решению. |
| **Сначала подобрать строй** / `tuned` | Нота ждёт результат worker; target Pitch Bend отправляется перед NoteOn. Задержка атаки переменная. |

Оба режима оставляют тяжёлый поиск вне audio callback. Фиксированной
host-declared задержки нет. В ожидающем режиме короткая нота может быть отменена,
если её отпустили до окончания поиска и sustain не удерживает её.

Offline render выполняет поиск синхронно и детерминированно. Он не воспроизводит
моменты завершения worker из realtime-сеанса; результат атаки и траектория
могут отличаться от live-воспроизведения. Подробнее: [Plugin guide](docs/plugin.md).

## Настройки, Scala и сохранение проекта

Изменения редактора — черновик. Нажмите зелёную **Validate and apply** в верхней
панели, чтобы применить и сохранить их в состоянии плагина. Применение настроек
отпускает текущие ноты. **Reload current settings** отменяет несохранённые правки;
**Reset to defaults** сбрасывает черновик — после него тоже нужен Apply.

Для импорта Scala:

1. Положите файлы в `~/.scales/` и выберите `.scl` / необязательный `.kbm` из списков,
   либо укажите произвольные пути. **Обновить список** перечитывает каталог.
   Отсутствующая папка допустима; подкаталоги не сканируются.
2. Без `.kbm` используется последовательный маппинг; `root_note` по умолчанию 60.
3. Нажмите **Import Scala into draft**, затем **Validate and apply**.
   **Clear scale / 12-TET base** возвращает стандартную базу.

Содержимое `.scl`, `.kbm` и пользовательского спектра хранится **внутри состояния
проекта**, а не только в виде путей. После переноса файлов или проекта настройка
восстанавливается без исходных файлов. Несохранённые черновики, звучащие голоса,
sustain, glide и кратковременные memory traces в проект не записываются.

Scala не отключает JI: optimizer подбирает отношения вокруг импортированных
базовых высот. Не все клавиши произвольного Scala-маппинга обязаны помещаться
в MIDI 0..127 плюс доступный bend; недоступные высоты не подменяются другими нотами.

## Быстрый старт: standalone

Дополнительно нужны ALSA development-библиотеки и `/dev/snd/seq`.
Для Debian/Ubuntu standalone-зависимости:

```sh
sudo apt install build-essential pkg-config libasound2-dev alsa-utils
cargo build --release -p adaptive-ji
./target/release/adaptive-ji --list-inputs
./target/release/adaptive-ji --input "KeyRig" -v
```

Подключите созданный источник **Adaptive JI Out** к MPE-инструменту.
Для входа от внешнего MIDI-трека вместо клавиатуры:

```sh
./target/release/adaptive-ji --virtual-input --bend-range 2 -v
```

Это создаёт **Adaptive JI In** (приёмник) и **Adaptive JI Out** (источник).
Связи настраиваются снаружи; внутренней петли нет. `--input` и `--virtual-input`
взаимоисключающие. Обычные ноты можно записывать/редактировать до адаптера,
а при воспроизведении строй пересчитывается заново.

После `cargo install --path .` бинарник доступен как `adaptive-ji`:

```sh
adaptive-ji --input "KeyRig" --scale /path/to/scale.scl --root-note 60
adaptive-ji --virtual-input --scale /path/to/scale.scl --kbm /path/to/map.kbm
adaptive-ji --scale /path/to/scale.scl --scale-info
adaptive-ji --help
aconnect -l
```

Standalone сохраняет синхронную политику: сначала подбор строя, затем атака.
Подробные CLI-примеры, диагностика и алгоритмы: [справочник](docs/standalone.md).

## Основные defaults

| Настройка | Значение |
| --- | --- |
| Prime limit / max complexity | 11 / 12 bits |
| Max deviation / anchor | ±50 cents / floating, ±35 cents |
| Candidates per note / beam width | 8 / 2048 |
| Harmonic / deviation / movement weights | 1 / 0.35 / 1.5 |
| Roughness / memory weights | 20 / 1 |
| Spectrum | harmonic, 12 partials, rolloff 1 |
| Glide / tick | 35 ms / 5 ms |
| Memory | relations, decay 0.80, reset on empty |
| Pitch-bend range | ±2 semitones |

Memory — экспериментальная кратковременная память отношений, не распознавание
тональности или аккордов. Она включена по умолчанию в этой версии;
`--memory-mode off` (или `Off` в плагине) её отключает.
Больший prime limit не означает «лучше»: он расширяет пространство отношений.

## Ограничения и производительность

- Максимум 15 одновременно живых голосов, включая удерживаемые педалью.
  Voice stealing отсутствует; лишние NoteOn игнорируются.
- Roughness — приближённая метрика синтетического спектра, не анализ аудио
  реального инструмента. Она предвычисляется до общего floating-anchor shift.
- Optimizer может быть медленным при больших спектрах/пространствах поиска.
  В плагине это задерживает улучшение строя, а в `tuned` — также атаки.
- В измеренном release-прогоне Ryzen 9 5900X поиск для 15 голосов занимал около
  16 ms, immediate callback — около 10–13 µs. Стресс-профиль с 128 partials
  давал около 800 ms поиска. Это примеры замеров, **не гарантия deadline**.
- MIDI panic и остановка транспорта очищают голоса. При разрушении графа без
  дальнейших process callbacks хост обязан сам завершить downstream-ноты.
- Поддержка note-effect output зависит от хоста; проверка формата не заменяет
  проверку конкретной MIDI-цепочки перед live-выступлением.

Подробности архитектуры, замеров и RT-границ: [realtime audit](docs/plugin-readiness.md).

## Разработка и проверка

```text
crates/core/     tuning, Scala, voices, sustain, memory, glide, panic
crates/runtime/ sample timeline и lock-free optimizer worker
crates/plugin/  CLAP/VST3, редактор, project state
src/            standalone CLI и ALSA/midir
examples/       диагностика отношений, roughness и производительности
xtask/          сборка plugin bundles
```

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
cargo run -p xtask -- bundle adaptive-ji-plugin --release
```

Обычные тесты не требуют MIDI-оборудования. ALSA loopback запускается отдельно:
`cargo test --test alsa_loopback -- --ignored` при доступном `/dev/snd/seq`.
Инструкции по форматным валидаторам: [Plugin guide](docs/plugin.md#performance-and-validation).

## Лицензия

Собственный код — [The Unlicense](UNLICENSE).
Сторонние компоненты и VST3 bindings сохраняют свои лицензии и условия
распространения: [THIRD_PARTY.md](THIRD_PARTY.md).
