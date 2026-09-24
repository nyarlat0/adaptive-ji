# adaptive-ji 0.4

CLI-программа для Linux: принимает обычный MIDI 1.0 через `midir`/ALSA,
подбирает контекстный 7-limit just intonation для всего аккорда с учётом
приближённой roughness между гармониками нот и выдаёт
MPE-подобный поток через виртуальный ALSA MIDI-порт **Adaptive JI Out**.
GUI, аудиодвижка, JACK API и плагинов в программе нет.

## Сборка и запуск

Нужны современный stable Rust с поддержкой edition 2024, `pkg-config`,
заголовки ALSA и доступ к ALSA sequencer (`/dev/snd/seq`). Например, Debian/Ubuntu:

```bash
sudo apt install build-essential pkg-config libasound2-dev alsa-utils
cargo build --release
./target/release/adaptive-ji --list-inputs
./target/release/adaptive-ji --input "KeyRig" --bend-range 2 --verbose
```

При желании установить бинарник в Cargo bin directory:

```bash
cargo install --path .
adaptive-ji --input "KeyRig"
```

`--input` принимает индекс из `--list-inputs` (начиная с **0**), полное имя
либо уникальную часть имени без учёта регистра. Полное совпадение приоритетнее
частичного. Если совпадений несколько, используйте индекс. Несуществующий вход
и неоднозначное имя завершают программу с понятной ошибкой.

```bash
adaptive-ji --list-inputs
adaptive-ji --input 0
adaptive-ji --input "KeyRig" --bend-range 2 -v
adaptive-ji --help
aconnect -l
```

`--bend-range` задаёт ±диапазон в полутонах, по умолчанию **2**. Допустимы
0.01–127.99 с шагом 0.01 для точного представления через RPN. Очень маленький
диапазон может обрезать JI-коррекцию. Обычный запуск показывает только предупреждения
и ошибки; `-v`/`--verbose` показывает NOTE ON/OFF, TUNE, reference и составляющие
adaptive score. `-vv` дополнительно показывает debug-сообщения об игнорируемом MIDI.
Фильтр можно переопределить переменной `RUST_LOG`.

По умолчанию используется новый optimizer. Для A/B-сравнения:

```bash
adaptive-ji --input "KeyRig" --strategy adaptive -v
adaptive-ji --input "KeyRig" --strategy adaptive --anchor-mode fixed -v
adaptive-ji --input "KeyRig" --strategy five-limit -v
```

Настройки optimizer (показаны defaults):

```bash
adaptive-ji --input "KeyRig" --strategy adaptive \
  --max-deviation 35 --candidates-per-note 8 --beam-width 2048 \
  --anchor-mode floating --anchor-max 35 \
  --w-harmonic 1 --w-deviation 0.35 --w-movement 1.5 --w-roughness 20 \
  --roughness-partials 12 --roughness-rolloff 1.0
```

`--max-deviation` принимает 0..100 cents, `--candidates-per-note` — 1..32,
`--beam-width` — 1..16384. Веса — конечные неотрицательные числа до 1000000.
`--anchor-mode` принимает `floating` (default) или `fixed`; `--anchor-max`
принимает 0..100 cents, default 35. Он ограничивает общий абсолютный сдвиг строя.
`--roughness-partials` — 1..64; `--roughness-rolloff` — 0.1..4.0.
Нулевые K/beam и NaN/бесконечности отклоняются до открытия MIDI-порта.
Более широкий beam и больше candidates увеличивают время пересчёта.
`--anchor-mode fixed` воспроизводит привязку reference к 0 cents из v0.3.
`--w-roughness 0` исключает pairwise roughness из поиска, но оставляет выбранный
anchor mode. Для сравнения именно с v0.2 используйте обе настройки:
`--anchor-mode fixed --w-roughness 0`.

## Surge XT standalone

```text
KeyRig 49 → adaptive-ji → Adaptive JI Out → Surge XT → audio output
```

1. Подключите клавиатуру и запустите `adaptive-ji --input "KeyRig" -v`.
2. В Surge XT включите **MPE** в области статуса. Через контекстное меню MPE
   или меню MPE Settings установите **MPE pitch bend range = 2 semitones**,
   либо ровно то значение, которое передали в `--bend-range`. Нужен именно
   диапазон MPE для отдельных нот. Настройку обязательно проверьте вручную:
   поддержка RPN зависит от принимающего инструмента.
3. В MIDI-настройках standalone выберите/подключите **Adaptive JI Out** как источник.
   Если требуется внешнее соединение, найдите порты командой `aconnect -l`
   и соедините источник adaptive-ji со входом Surge XT через `aconnect SOURCE DEST`.
   `SOURCE` и `DEST` замените реальными адресами `client:port`, например `128:0`.
4. Отключите прямое подключение KeyRig 49 к Surge XT, иначе будут звучать также
   исходные ненастроенные ноты. Выберите полифонический патч с достаточно долгим
   sustain и обычным исходным строем 12-TET; отключите дополнительный microtuning.

Для первой проверки сыграйте MIDI **60, 64, 67** (C4, E4, G4 в принятой здесь
нумерации). Отношения станут примерно C:E:G = 4:5:6. При первом одиночном C
offset будет 0; в аккорде общий floating anchor может немного сдвинуть C и
соответственно E/G. Добавьте **Bb4, MIDI 70**: optimizer выберет гармоническую
септиму 7/4, получив 4:5:6:7.
Удерживая E/G/Bb, отпустите C: reference станет E, удерживаемые голоса получат
новые bend без retrigger и останутся почти на тех же абсолютных высотах.
В логах Bb обозначается как A#.

В adaptive добавление/удаление любой ноты может изменить решение: все пары
участвуют в score. Movement penalty может сохранить прежние offsets, если
перестройка не окупает движение. В `--strategy five-limit` действует прежнее
правило: самая низкая нота — root, поэтому добавление более высокой ноты
не меняет настройку остальных.

Surge XT позволяет отдельно настроить сглаживание MPE pitch bend. Программа
отправляет изменения сразу, собственного glide нет. Настройки MPE описаны
в [официальном руководстве Surge XT](https://surge-synthesizer.github.io/manual-xt/#mpe-settings).

## Ardour + Surge XT plugin

```text
KeyRig 49
  ↓
adaptive-ji
  ↓
Adaptive JI Out (ALSA MIDI source)
  ↓
Ardour MIDI track
  ↓
Surge XT plugin
  ↓
audio output
```

1. Запустите adaptive-ji и Ardour с доступом к ALSA MIDI.
2. Создайте MIDI track, добавьте Surge XT как инструмент.
3. Подключите **Adaptive JI Out** ко входу MIDI track через настройки входа/
   MIDI connections Ardour. В конфигурации с JACK/PipeWire может потребоваться
   ALSA MIDI bridge, предоставляемый вашей аудиосистемой: adaptive-ji создаёт
   именно ALSA sequencer port и сам JACK-порты не создаёт.
4. Сохраняйте исходные MIDI channels: принимайте все каналы, отключите принудительное
   сведение/переназначение всех событий на один канал. Иначе независимые bend потеряются.
5. Включите мониторинг входа трека (при необходимости arm для записи), а в Surge XT —
   MPE и соответствующий `--bend-range` диапазон. Исключите прямой вход клавиатуры.
6. Повторите проверку C–E–G с добавлением Bb и отпусканием C.

Если порта нет, проверьте `adaptive-ji --list-inputs`, `aconnect -l`, настройки
MIDI backend/bridge в аудиосистеме и доступ процесса к `/dev/snd/seq`.
Виртуальный порт существует только пока запущен adaptive-ji. После перезапуска
соединение может потребоваться создать заново. `aconnect -l` проверяет соединения,
а `-v` показывает рассчитанные ноты и bend.

## Алгоритм и MIDI-поведение

### AdaptiveRationalOptimizer (по умолчанию)

Reference — самый старый активный voice (минимальный монотонный ID allocator),
а не lowest note или распознанный root. Его **rational ratio всегда 1/1**,
но в floating mode его фактический offset равен общему `anchor_cents` и может
быть отличен от нуля. Остальные ratios задают интервалы относительно него.
Например, если сначала сыграть E4, затем G4 и C5, решение может быть
E=1/1, G=6/5, C=8/5; интервал C/G=4/3. Общий anchor не меняет эти отношения.

`Ratio` хранит положительные сокращённые `u64` numerator/denominator. Умножение,
деление и масштабирование на степени двойки проверяют переполнение и выполняют
сокращение до умножения. Для генерации перебираются степени 3: −4..4, 5: −3..3,
7: −2..2 (315 сочетаний); степень двойки выбирается под MIDI-интервал, включая
отрицательные интервалы и несколько октав.

Candidates фильтруются по `abs(base_offset) <= max_deviation`, затем ранжируются по
`w_harmonic * complexity(ratio) + w_deviation * (offset/25)^2`, и остаются лучшие K.
При равенстве используются абсолютное отклонение и целочисленные numerator/denominator.
Это только предварительное сокращение набора: итоговые harmonic и roughness
score считают **все** пары с одинаковым весом, без особого преимущества reference.
При default K=8 генератор сохраняет 5/4, 6/5, 3/2 и 7/4 в соответствующих
интервалах. Roughness не используется для отсеивания относительно reference.

При пустом окне используется ближайший ratio из конечного 7-limit набора с warning
один раз при генерации этого интервала. Это явно допускает выход за `max-deviation`:
например, нулевое окно не может точно представить большинство интервалов 12-TET
рациональными числами. При defaults fallback для MIDI-интервалов −127..127 не нужен.

Для каждого candidate сначала считается `base_i = cents(ratio_i) −
100·(midi_i − midi_reference)`. Фактический `offset_i = anchor + base_i`.
Для reference base=0, но actual offset не обязан быть нулём.

Допустимый anchor для полного или частичного assignment — пересечение:

```text
[-anchor_max, +anchor_max]
∩ [-max_deviation - base_i, +max_deviation - base_i] для каждого обычного candidate i
```

При `--anchor-mode fixed` anchor строго 0 и старый score v0.3 сохраняется.
При floating ограничения гарантируют `abs(actual_offset_i) <= max_deviation`
для обычных candidates. Fallback за пределами невозможного узкого candidate
окна сохраняет документированное исключение v0.3: для него этот предел не
применяется, иначе при некоторых допустимых настройках решения не существует.

Deviation и movement зависят от anchor квадратично. Beam state хранит число
voices `N`, число ранее звучавших `H`, суммы `Σbase`, `Σbase²`,
`Σ(base−previous)` и `Σ(base−previous)²`. Для каждого partial assignment:

```text
Q(anchor) = A*anchor² + B*anchor + C
A = w_deviation*N/625 + w_movement*H/100
B = 2*(w_deviation*Σbase/625 + w_movement*Σ(base−previous)/100)
C = w_deviation*Σbase²/625 + w_movement*Σ(base−previous)²/100
anchor_opt = clamp(-B/(2*A), feasible_interval)   при A > 0
```

Если A=0, выбирается ближайший к нулю допустимый anchor. Это абсолютная
оптимизация относительно 12-TET на **каждом** retune: прежний anchor не
накапливается и не добавляется инкрементально. После отпускания всех нот
история очищается, новая одиночная нота снова имеет offset=0.
Оптимизированный Q участвует в отсечении partial beam assignments, то есть
ratios и anchor выбираются совместно.

Для сокращённого p/q удаляются все степени двойки из обеих частей:

```text
complexity(p/q) = log2(odd_num) + log2(odd_den)

harmonic = sum over i < j: complexity(ratio_j / ratio_i)
deviation = sum: ((anchor + base_offset) / 25)^2
movement = sum over previously held voices: ((anchor + base_offset - previous_offset) / 10)^2
roughness = sum over i < j: R(spectrum_i, spectrum_j)
score = w_harmonic * harmonic + w_deviation * deviation
      + w_movement * movement + w_roughness * roughness
```

Например, complexity(3/2)=log2(3), complexity(6/5)=log2(15); октавы не меняют
сложность. Новая нота не получает movement penalty. Предыдущие offsets хранятся
по voice ID, а не MIDI note; отпущенные экземпляры удаляются из истории.

Beam search сначала назначает reference, затем остальные voices по MIDI note и ID.
На каждом шаге добавляются индивидуальные costs и парные costs со всеми уже
назначенными voices, после чего сохраняются лучшие `beam-width` вариантов.
Каждая unordered pair учитывается ровно один раз. Все tie-breaks детерминированы.
Парные `PairCost { harmonic, roughness }` и candidate spectra заранее вычисляются;
в цикле расширения нет gcd/log2, exp, pow и генерации спектров. Roughness v0.4
сознательно рассчитывается **до** общего anchor shift: он немного меняет
абсолютные частоты, поэтому roughness после сдвига здесь приближена. Это сохраняет
быструю pairwise precomputation; более точный пересчёт оставлен на будущую версию.
Сложность порядка O(beam × K × N²), плюс отбор/сортировка beam; полный K^N не перебирается.
Beam search и отсечение candidates приближённые: глобальный минимум не гарантируется.

Ratios по умолчанию для C–E–G–Bb, свежего и последовательного аккорда:

```text
C4   1/1    anchor
E4   5/4    anchor - 13.686 cents
G4   3/2    anchor + 1.955 cents
Bb4  7/4    anchor - 31.174 cents
```

История может изменять выбор в других последовательностях — это назначение
movement term. В fixed mode после отпускания C из C–E–G при `--w-roughness 0`
optimizer v0.2 предпочитает G/E=32/27: G перемещается примерно на 7.820 cents.
При `--w-movement 0` выбирается более простой 6/5 с движением G на 13.686 cents.
Новая модель для E4–G4 даёт roughness около 0.08217 для 6/5 и 0.09325 для
32/27 при default spectrum. Достаточно большой `--w-roughness`, например 200,
может перевесить движение и выбрать менее грубый интервал 6/5. Обычный вес
20 оставляет канонические аккорды 4:5:6 и 4:5:6:7.

**Смена reference:** в floating mode новый старейший voice может сохранить
прежний абсолютный offset: его ratio становится 1/1, а anchor подбирается близко
к предыдущей высоте. После C–E–G → E–G система E:G=1:6/5 способна сохранить
обе высоты почти без движения. После C–E–G–Bb → E–G–Bb аналогично сохраняется
1:6/5:7/5. В fixed mode новый reference снова принудительно получает offset=0.

`Costs` хранит harmonic/deviation/movement/roughness раздельно; в `-v` видны
все четыре слагаемых и общий score. В расчёте используются абсолютные частоты:

```text
f0 = 440 * 2^((midi_note - 69 + offset_cents/100)/12)
f_n = n * f0
a_n_raw = 1 / n^rolloff
a_n = a_n_raw / sqrt(sum(a_k_raw^2))
```

Амплитуды имеют суммарную энергию 1 при любом количестве partials; дополнительного
скрытого множителя нет. Для каждой пары partials **разных** нот:

```text
f_min = min(f1, f2)
delta = abs(f1 - f2)
s = 0.24 / (0.0207 * f_min + 18.96)
x = s * delta
r_pair = a1 * a2 * (exp(-3.5*x) - exp(-5.75*x))
R(noteA, noteB) = sum(r_pair over cross-note partial pairs)
```

Мелкие отрицательные ошибки округления обрезаются до 0. Это приближённая
Sethares/Plomp–Levelt-style метрика сенсорной диссонантности синтетического
гармонического спектра, не физически точная модель конкретного патча Surge XT.
FFT, анализ аудио, inharmonicity фортепиано, распознавание аккордов и тональности
не используются.

Для просмотра кандидатов без MIDI:

```bash
cargo run --release --example roughness_scan -- 60 67
cargo run --release --example roughness_scan -- 64 67
```

Выводятся ratio, cents, **base offset**, harmonic complexity и roughness относительно
первой ноты, принятой за reference с anchor 0. Пример показывает индивидуальные
парные значения; итоговый optimizer считает пары **всех** одновременно звучащих
нот и также учитывает движение, отклонение от 12-TET и floating anchor.

### FiveLimitLowestRoot (сохранённый baseline v0.1)

`--strategy five-limit` выбирает простой baseline. Root —
минимальная активная MIDI note; её отношение всегда 1/1 и коррекция 0 cents.
Повторные экземпляры одинаковых нот участвуют независимо, с одинаковой высотой.

| Полутонов от root по модулю 12 | Ratio |
| --- | --- |
| 0 | 1/1 |
| 1 | 16/15 |
| 2 | 9/8 |
| 3 | 6/5 |
| 4 | 5/4 |
| 5 | 4/3 |
| 6 | 45/32 |
| 7 | 3/2 |
| 8 | 8/5 |
| 9 | 5/3 |
| 10 | 9/5 |
| 11 | 15/8 |

Для `d = note - root` отношение умножается на `2^(d / 12)` (целочисленное
деление), затем `offset = 1200 * log2(ratio) - d * 100`.

```text
C4: 1/1     +0.000 cents
E4: 5/4    -13.686 cents
G4: 3/2     +1.955 cents
```

Pitch bend вычисляется как `round(8192 + offset / (bend_range * 100) * 8192)`
с ограничением 0..16383. Ratio в логах включает октавы и сокращается.

- Channel 1 зарезервирован как master; 15 голосов занимают channels 2..16.
  В коде и MIDI-байтах нумерация 0..15, в логах — 1..16.
- Перед NoteOn нового голоса отправляется его Pitch Bend. Каждое добавление/
  удаление активного голоса пересчитывает все оставшиеся voices и отправляет
  их bend без повторного NoteOn.
- NoteOff использует назначенный канал, освобождает его и возвращает bend в 8192.
  NoteOn с velocity 0 равнозначен NoteOff. Release velocity сохраняется.
- Повторные NoteOn одной ноты создают отдельные голоса. NoteOff снимает самый
  ранний ещё не отпущенный экземпляр (**FIFO отдельно для каждой пары input
  channel + note**). Это соглашение необходимо, поскольку MIDI 1.0 не передаёт ID.
- При 15 занятых каналах новый NoteOn игнорируется с warning; voice stealing нет.
  Его последующий NoteOff также учитывается в FIFO и не снимает более поздний голос.
- RPN 0 Pitch Bend Sensitivity (CC101/100, CC6/38, затем RPN Null) отправляется
  на channels 2..16 при старте и повторно перед выделенным голосом, чтобы
  настройку получили и подключившиеся позже клиенты. Включать MPE нужно в синтезаторе.
- CC64 и остальные CC транслируются на все member channels, включая свободные:
  новый голос получает текущее состояние педали, а pedal-up достигает уже
  освобождённых каналов. CC120/123 дополнительно обрабатываются как общий panic:
  сбрасываются голоса, sustain и bend.
- Входящие Pitch Bend, aftertouch, Program Change и системные/SysEx сообщения
  игнорируются с debug log. Входящие CC настройки диапазона могут изменить
  состояние синтезатора: во время игры не посылайте противоречащие настройкам RPN.

**Сохранённое ограничение sustain:** это только CC passthrough. Tuning и allocator
отслеживают физически удерживаемые клавиши, а не продолжение звука под педалью.
На NoteOff канал немедленно освобождается и центрируется; pedal-held нота и
release tail могут изменить высоту при сбросе/повторном использовании канала.
Для первой проверки точного строя играйте без педали. Полноценного управления
педальными голосами в этой версии нет.

При Ctrl-C (также SIGTERM/SIGHUP) вход закрывается, отправляются NoteOff для
активных голосов, CC64=0 и CC123 на channels 1..16, затем центрирование bend
на channels 2..16. Та же очистка выполняется при ошибке отправки/переполнении
очереди. Даже после отдельной ошибки отправки программа пробует остальные
сообщения очистки. SIGKILL или физический разрыв MIDI-соединения не позволяют
гарантировать доставку; в таком случае используйте panic синтезатора.

Обработка событий последовательная, callback передаёт их в ограниченную очередь
без ожидания свободного места. При переполнении программа завершает работу с
очисткой голосов вместо пропуска отдельных NoteOff. Это приложение для игры
в реальном времени без гарантий hard realtime; verbose-логирование может добавлять
нагрузку. Отключение USB-клавиатуры ALSA не всегда сообщает callback: если вход
перестал отвечать, завершите процесс Ctrl-C, подключите клавиатуру и запустите заново.

## Структура и проверки

```text
src/
  main.rs                CLI и logging
  lib.rs                 библиотечные модули
  midi.rs                midir/ALSA transport, parser, выбор входа, shutdown
  mpe.rs                 обработчик событий и генерация MIDI-сообщений
  voice.rs               Voice, FIFO экземпляров, allocator каналов
  pitch_bend.rs          чистая математика bend и RPN
  tuning/
    mod.rs               ActiveNote, TunedNote, TuningStrategy
    rational.rs          reduced Ratio, checked arithmetic, complexity
    candidates.rs        7-limit lattice, pruning и кэш по интервалам
    adaptive.rs          config, Costs, floating anchor, история voices, beam search
    roughness.rs         гармонические partials и pairwise sensory roughness
    five_limit.rs        FiveLimitLowestRoot
examples/
  retune_benchmark.rs     диагностика времени пересчёта 15 voices
  roughness_scan.rs       сравнение roughness кандидатов для двух MIDI notes
```

Следующий алгоритм реализует `TuningStrategy::tune(&mut self, &[ActiveNote]) ->
Vec<TunedNote>` и возвращает результат по каждому voice ID. Транспорт менять
не требуется. Tuning не зависит от midir или MIDI output.

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
cargo build --release
```

Тесты без MIDI-оборудования проверяют математику, отношения/октавы, root changes,
очерёдность сообщений, отсутствие retrigger, повторные ноты, переполнение,
CC и сообщения очистки, а также рациональную арифметику, генерацию candidates,
4:5:6, 4:5:6:7, обращения, movement penalty, детерминизм и 15 voices.
Roughness покрыта тестами спектра, нормализации, симметрии, простых интервалов,
границ параметров, A/B E–G и влияния веса на выбор.
Floating anchor проверен аналитической минимизацией против плотной сетки,
ограничением диапазона, сохранением E/G и E/G/Bb после смены reference,
повторными voice IDs и сбросом истории после полного отпускания нот. Fixed mode
проверяется прежними каноническими тестами v0.3.
Для маленького аккорда beam search сверяется с отдельным полным перебором.
Отдельный интеграционный тест требует доступного ALSA
sequencer, но не физической клавиатуры или синтезатора:

```bash
cargo test --test alsa_loopback -- --ignored
```

Он создаёт временный виртуальный источник и монитор, запускает настоящий бинарник,
проверяет five-limit, adaptive fixed, adaptive floating и floating с
`--w-roughness 0` через ALSA, включая C–E–G, септиму, смену reference/root без
retrigger, непрерывность bend и очистку при SIGINT. По умолчанию этот тест
пропущен, чтобы `cargo test` работал без
`/dev/snd/seq`. Звучание в standalone/Ardour проверяется вручную по сценариям выше.

### Performance sanity check

```bash
cargo run --release --example retune_benchmark -- 100
```

Без жёстких timing assertions. Пример на AMD Ryzen 9 5900X, release,
K=8/beam=2048, `w_roughness=20`, 30 повторов плотного 15-голосного набора:

| Сценарий | Fixed среднее | Floating среднее |
| --- | ---: | ---: |
| Свежий аккорд | 11.990 мс | 12.216 мс |
| Удерживаемые 15 voices | 11.612 мс | 12.194 мс |
| Добавление/отпускание 15-го voice | 11.063 мс | 11.657 мс |

Измеряется только `tune()`, без создания optimizer, MIDI I/O и логирования.
Это ориентир конкретного запуска, а не гарантия задержки; параметры поиска,
нагрузка CPU и verbose output влияют на фактическое время.
