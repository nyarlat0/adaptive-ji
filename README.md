# adaptive-ji 1.1.0

CLI-программа для Linux: принимает обычный MIDI 1.0 через `midir`/ALSA,
подбирает контекстный adaptive just intonation с configurable arbitrary
prime limit для всего аккорда с учётом
приближённой roughness между partials выбранного спектра и выдаёт
MPE-подобный поток через виртуальный ALSA MIDI-порт **Adaptive JI Out**.
GUI, аудиодвижка, JACK API и плагинов в программе нет.

В 1.1 Scala `.scl` с `.kbm` или без него задаёт **базовые высоты клавиш**.
Поверх них тот же adaptive optimizer продолжает выбирать рациональные отношения
всего аккорда, учитывать roughness, память и движение, а затем плавно перестраивать
уже звучащие голоса. Scala — ориентир, а не запрет на JI-коррекцию.

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

### Temporal movement feedback (v0.9)

Movement penalty относится к фактически слышимой позиции voice: перед каждым
retune один monotonic snapshot интерполированных pitch offsets передаётся в
optimizer. Поэтому mid-glide событие оценивает движение от текущей высоты, а не
от недостигнутого прошлого target. Тот же snapshot используется как начало
нового glide; ticks только интерполируют уже выбранные targets и не запускают
beam search повторно. Для новой ноты `current_offset_cents=None`, поэтому она
не получает movement penalty.
Фильтр можно переопределить переменной `RUST_LOG`.

`--glide-ms 35` (default, 0..=2000) плавно перестраивает уже звучащие голоса
линейно в центах; `--glide-tick-ms 5` (1..=100) задаёт шаг отправки bend.
Новая нота всегда получает конечный bend **до** NoteOn, без glide. `--glide-ms 0`
возвращает немедленные bend v0.5. При длительности меньше шага финальная точка
всё равно отправляется вовремя; одинаковые округлённые значения не повторяются.

По умолчанию используется новый optimizer. Для A/B-сравнения:

```bash
adaptive-ji --input "KeyRig" --strategy adaptive -v
adaptive-ji --input "KeyRig" --strategy adaptive --anchor-mode fixed -v
adaptive-ji --input "KeyRig" --strategy five-limit -v
adaptive-ji --input "KeyRig" --spectrum bright -v
adaptive-ji --input "KeyRig" --spectrum-file spectrum.toml -v
adaptive-ji --input "KeyRig" --prime-limit 11 --max-deviation 60 -v
adaptive-ji --input "KeyRig" --memory-mode relations --w-memory 1.0 -v
adaptive-ji --input "KeyRig" --memory-mode off -v
```

Настройки optimizer (показаны defaults):

```bash
adaptive-ji --input "KeyRig" --strategy adaptive \
  --max-deviation 50 --candidates-per-note 8 --beam-width 2048 \
  --prime-limit 11 \
  --max-complexity 12 \
  --anchor-mode floating --anchor-max 35 \
  --w-harmonic 1 --w-deviation 0.35 --w-movement 1.5 --w-roughness 20 \
  --roughness-partials 12 --roughness-rolloff 1.0
```

`--max-deviation` принимает 0..100 cents, `--candidates-per-note` — 1..32,
`--beam-width` — 1..16384. Веса — конечные неотрицательные числа до 1000000.
`--anchor-mode` принимает `floating` (default) или `fixed`; `--anchor-max`
принимает 0..100 cents, default 35. Он ограничивает общий абсолютный сдвиг строя.
`--roughness-partials` — 1..64; `--roughness-rolloff` — 0.1..4.0.
`--spectrum` выбирает `harmonic` (default), `sine`, `saw`, `square`, `soft`
или `bright`. Для built-in `--roughness-partials` задаёт максимальный номер
гармоники (для sine всегда один partial); `--roughness-rolloff` меняет показатель
спада: harmonic/saw/square ×1, soft ×2, bright ×0.5. При default rolloff=1
это соответственно `1/n`, `1/n²` и `1/√n`; square оставляет только нечётные `n`.
`--spectrum-file PATH` загружает статический custom TOML и имеет приоритет над
`--spectrum`: оба generator-параметра при этом игнорируются. Максимум 128
partials, превышение выдаёт ошибку без усечения.
Нулевые K/beam и NaN/бесконечности отклоняются до открытия MIDI-порта.
Более широкий beam и больше candidates увеличивают время пересчёта.
`--prime-limit` принимает любой простой предел от 3 до 997 (CLI default 11):
p-limit разрешает все простые множители `<= p`. 5-limit использует 2/3/5,
7-limit добавляет septimal 7, 11-limit — undecimal 11, а 13/17 и выше
расширяют пространство аналогично. Более высокий предел не означает «лучше».
Кандидаты ищутся bounded best-first поиском по modified Tenney complexity;
полного Cartesian произведения exponent axes нет. `--max-complexity` (default
12) задаёт budget этого поиска.
В 1.1 по умолчанию включён экспериментальный `--memory-mode relations`:
он хранит до нескольких octave-equivalent rational
relations между MIDI pitch classes. Настройки: `--w-memory 1.0`,
`--memory-decay 0.80`, `--memory-reset-on-empty true`. Это не chord/key
recognition и не ML: memory хранит sparse prime-exponent fingerprints, а не
absolute cents, anchor, glide или MIDI channels. C# и Db намеренно совпадают.
Memory обновляется после финального assignment, но не на glide ticks.
Это даёт больше гармонической зависимости от истории; при желании отключите
её флагом `--memory-mode off` — без lookup, cost и обновления памяти.
CLI defaults также расширены с 7-limit/35c до 11-limit/50c при прежнем K8, чтобы
undecimal-кандидаты вроде 11/8 действительно могли попадать в поиск.
Более широкий поиск не делает каждый результат «лучше»; для прежнего
консервативного профиля укажите `--prime-limit 7 --max-deviation 35
--candidates-per-note 8 --memory-mode off`.

## Импорт Scala `.scl` / `.kbm`

```bash
adaptive-ji --input "KeyRig" --scale scales/albionbyz.scl -v
adaptive-ji --input "KeyRig" \
  --scale "scales/13-limit scale from Brendan Byrnes Operator.scl" -v
adaptive-ji --input "KeyRig" --scale scales/albionbyz.scl --root-note D4 -v
adaptive-ji --input "KeyRig" --scale some.scl --kbm custom.kbm -v
adaptive-ji --scale-info --scale scales/albionbyz.scl
```

Если рядом с `.scl` есть одноимённый `.kbm`, он подхватывается автоматически.
`--kbm` выбирает другой файл явно. `--no-kbm` отключает автопоиск; `--root-note`
(MIDI 0–127 либо `C4`, `F#3`, `Bb2` и т. п.) тоже использует маппинг **без**
`.kbm`. При отсутствии карты последовательные клавиши идут по последовательным
ступеням от degree 0 на C4/MIDI 60, повторяясь через период, указанный
последней ступенью `.scl`. Частота root при таком маппинге равна его обычной
12-TET частоте при A4=440 Hz. С `.kbm` учитываются границы диапазона, middle
key, reference MIDI note/frequency, formal octave degree, повторяющийся pattern
и `x` для немаппированных клавиш. Клавиши вне `.kbm` диапазона имеют
12-TET базовую высоту; `x` не звучит. `--scale-info` печатает базовую карту
без ALSA; это **не** итоговые высоты, зависящие от текущего аккорда.

Для каждой ступени программа выбирает ближайший MIDI NoteOn и оставляет только
остаток в Pitch Bend; выбор MIDI NoteOn сохраняется на всю жизнь голоса, включая
NoteOff, sustain и panic. Для adaptive коррекции резервируется пространство
`--max-deviation` внутри `--bend-range`. Если высота недостижима (обычно на
краях MIDI 0–127 для неоктавных шкал), клавиша не звучит и при загрузке
выводится предупреждение с **входными** MIDI-диапазонами доступных и недоступных
клавиш. Оно относится ко всей карте из 128 MIDI-номеров: программа не знает
физический диапазон подключённой клавиатуры. `--scale-info` показывает также
недоступные клавиши, их требуемые частоты и отдельно пропуски `.kbm` (`x`).

Например, `13 equal divisions of 3_1.scl` с одноимённой `.kbm` задаёт шаг
146.304231¢ и тритаву 1901.955001¢ на каждые 13 клавиш. При стандартных
`--bend-range 2 --max-deviation 50` доступны входные MIDI **18–106**;
39 недоступных номеров — **0–17 и 107–127**. Весь диапазон 36–96 работает
при ±2 semitones, включая резерв для adaptive JI. Увеличивать bend range
из-за одного этого сообщения не требуется. Крайние ноты карты действительно
выходят за высоты MIDI 0–127: входная нота 127 здесь требует высоты примерно
MIDI 158.024. Более широкий bend range расширяет доступные края, но менять
его имеет смысл, только если нужны именно эти ноты; такой же диапазон должен
быть установлен в синтезаторе.

Именно Scala-интервалы служат центрами candidate windows: deviation считает
отклонение от импортированной высоты, а floating anchor привязывается к Scala
высоте старейшего голоса. `--prime-limit`, harmonic complexity, roughness,
movement, memory и beam search продолжают работать. Например, если `.scl`
задаёт E4 на −20c относительно 12-TET, оптимизатор может выбрать чистое 5/4
около −13.686c, когда это улучшает аккорд. При `--scale` используется
`--strategy adaptive`; старый `five-limit` baseline с ним не совмещается.

Формат `.scl`: комментарии начинаются с `!`, первый текст — описание, затем
число ступеней и значения в cents (`386.31`) или отношениях (`5/4`). Последнее
значение — период и может быть не 2/1. Формат `.kbm` задаёт отдельную
клавиатурную карту; подробности есть в
[описании Scala scale format](https://www.huygens-fokker.org/scala/scl_format.html)
и [Scala keyboard mappings](https://www.huygens-fokker.org/scala/help.htm#mappings).
Без Scala `--anchor-mode fixed` воспроизводит привязку reference к 0 cents
из v0.3; со Scala он фиксирует anchor на базовой высоте reference.
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

Surge XT позволяет отдельно настроить сглаживание MPE pitch bend. При
`--glide-ms > 0` его лучше отключить или существенно уменьшить, чтобы избежать
двойного сглаживания. При `--glide-ms 0` можно снова полагаться на сглаживание
синтезатора. Настройки MPE описаны
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
сокращение до умножения. Кандидаты генерируются bounded best-first поиском
по нечётным простым множителям до выбранного prime limit и complexity budget;
степень двойки подбирается под MIDI-интервал, включая отрицательные интервалы
и несколько октав.

Пусть `scala_i` — базовый offset клавиши от 12-TET (0 без `.scl`). Candidates
фильтруются по `abs(base_offset - (scala_i - scala_reference)) <= max_deviation`,
затем ранжируются по harmonic complexity и отклонению от этого Scala-интервала;
остаются лучшие K.
При равенстве используются абсолютное отклонение и целочисленные numerator/denominator.
Это только предварительное сокращение набора: итоговые harmonic и roughness
score считают **все** пары с одинаковым весом, без особого преимущества reference.
При CLI default K=8 генератор сохраняет важные 5/4, 6/5, 3/2 и 7/4 в соответствующих
интервалах. Roughness не используется для отсеивания относительно reference.

При пустом окне используется ближайший ratio из конечного выбранного prime-limit набора с warning
один раз при генерации этого интервала. Это явно допускает выход за `max-deviation`:
например, нулевое окно не может точно представить большинство интервалов 12-TET
рациональными числами. При defaults fallback для MIDI-интервалов −127..127 не нужен.

Для каждого candidate сначала считается `base_i = cents(ratio_i) −
100·(midi_i − midi_reference)`. Фактический `offset_i = anchor + base_i`.
Для reference base=0, но actual offset не обязан быть нулём.

Допустимый anchor для полного или частичного assignment — пересечение:

```text
[scala_reference - anchor_max, scala_reference + anchor_max]
∩ [scala_i - max_deviation - base_i,
   scala_i + max_deviation - base_i] для каждого обычного candidate i
```

При `--anchor-mode fixed` anchor равен `scala_reference` (0 без Scala).
При floating ограничения гарантируют `abs(actual_offset_i - scala_i) <= max_deviation`
для обычных candidates. Fallback за пределами невозможного узкого candidate
окна сохраняет документированное исключение v0.3: для него этот предел не
применяется, иначе при некоторых допустимых настройках решения не существует.

Deviation и movement зависят от anchor квадратично. Beam state хранит число
voices `N`, число ранее звучавших `H`, суммы `Σbase`, `Σbase²`,
`Σ(base−current)` и `Σ(base−current)²`. Для каждого partial assignment:

```text
Q(anchor) = A*anchor² + B*anchor + C
A = w_deviation*N/625 + w_movement*H/100
B = 2*(w_deviation*Σbase/625 + w_movement*Σ(base−current)/100)
C = w_deviation*Σbase²/625 + w_movement*Σ(base−current)²/100
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
deviation = sum: ((anchor + base_offset - scala_offset) / 25)^2
movement = sum over sounding voices: ((anchor + base_offset - current_audible_offset) / 10)^2
roughness = sum over i < j: R(spectrum_i, spectrum_j)
score = w_harmonic * harmonic + w_deviation * deviation
      + w_movement * movement + w_roughness * roughness
      + w_memory * memory
```

Например, complexity(3/2)=log2(3), complexity(6/5)=log2(15); октавы не меняют
сложность. Новая нота не получает movement penalty. Предыдущие offsets хранятся
по voice ID, а не MIDI note; отпущенные экземпляры удаляются из истории.
Начиная с v0.9 optimizer сравнивает с фактически интерполированной слышимой
позицией на момент retune; тот же snapshot служит началом нового glide.

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

Candidate generator выполняет bounded best-first expansion odd-prime ratios.
Каждый ненулевой exponent увеличивает modified Tenney complexity, поэтому
при finite `max-complexity` очередь конечна; branches выше budget не
расширяются. После octave normalization кандидаты фильтруются по
`max-deviation`, сортируются по harmonic/deviation cost и сокращаются до K;
полный Cartesian search не выполняется. Кэш разделён по `(MIDI interval,
prime limit, max-complexity)`. Внутренний защитный cap поиска — 50 000
посещённых ratio states на profile; diagnostics `ratio_scan` показывает размер
полученного bounded pool.

Классический undecimal пример `11/8 = 551.318 cents`. Для tritone MIDI 60→66
это offset примерно `-48.682 cents`: новый CLI default `max-deviation=50`
допускает его, а прежний консервативный `35` исключал. Для диагностики
можно использовать `--max-deviation 60`.
Для пары 60→66 roughness scan показывает profile-dependent ranking: sine даёт
примерно `16/11=0.0154` против `11/8=0.0301`, а bright — `11/8=0.0994`
против `16/11=0.1018`. Это следствие cross-partial model, а не специальное
правило для undecimal ratios.

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

`Costs` хранит harmonic/deviation/movement/roughness/memory раздельно; в `-v` видны
все четыре слагаемых и общий score. В расчёте используются абсолютные частоты:

```text
f0 = 440 * 2^((midi_note - 69 + offset_cents/100)/12)
f_i = frequency_ratio_i * f0
a_i = amplitude_i / sqrt(sum(amplitude_k^2))
```

Амплитуды имеют суммарную энергию 1 при любом количестве partials; дополнительного
скрытого множителя нет. Для harmonic-default `frequency_ratio=n` и
`a_raw=1/n^rolloff`; custom profile разрешает дробные отношения вроде `4.03`.
Для каждой пары partials **разных** нот:

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
спектра, не физически точная модель конкретного патча Surge XT.
Roughness **не является свойством одного ratio**: она зависит от partials тембра.
Например, `roughness_scan -- 60 67` даёт для sine `3/2 ≈ 0.010414`,
`32/21 ≈ 0.008465`, а для bright — `3/2 ≈ 0.047109`, `32/21 ≈ 0.087249`.
Это разные ranking при одной паре MIDI-нот; с большим `--w-roughness`
optimizer может выбрать разный строй. Sine не получает преимуществ от
совпадающих верхних гармоник, потому что их нет. FFT, анализ аудио,
автоматическое определение патча, распознавание аккордов и тональности
не используются.

Custom TOML (один статический профиль на запуск):

```toml
[[partial]]
ratio = 1.0
amplitude = 1.0

[[partial]]
ratio = 2.001
amplitude = 0.52

[[partial]]
ratio = 4.03
amplitude = 0.18
```

`ratio` должен быть конечным и >0, `amplitude` — конечной и ≥0; нулевые
amplitudes удаляются, остальные нормируются до суммарной энергии 1.
Нужен хотя бы один ненулевой partial. Никакого hot reload или спектра по
отдельным нотам/velocity пока нет. `-v` показывает имя профиля и число partials
один раз при запуске.

Для просмотра кандидатов без MIDI:

```bash
cargo run --release --example roughness_scan -- 60 67
cargo run --release --example roughness_scan -- 60 67 --spectrum sine
cargo run --release --example roughness_scan -- 60 67 --spectrum bright
cargo run --release --example ratio_scan -- 60 66 --prime-limit 7 --max-deviation 60
cargo run --release --example ratio_scan -- 60 66 --prime-limit 11 --max-deviation 60 --spectrum bright
cargo run --release --example ratio_scan -- 60 66 --prime-limit 13 --max-deviation 60
cargo run --release --example spectrum_dump -- bright
cargo run --release --example spectrum_dump -- --spectrum-file spectrum.toml
```

Выводятся ratio, cents, **base offset**, harmonic complexity и roughness относительно
первой ноты, принятой за reference с anchor 0. Пример показывает индивидуальные
парные значения; итоговый optimizer считает пары **всех** одновременно звучащих
нот и также учитывает движение, отклонение от 12-TET и floating anchor.

### FiveLimitLowestRoot (сохранённый baseline v0.1)

`--strategy five-limit` выбирает простой baseline. Root —
минимальная активная MIDI note; её отношение всегда 1/1 и коррекция 0 cents.
Повторные экземпляры одинаковых нот участвуют независимо, с одинаковой высотой.

Это не то же самое, что `--prime-limit 5`: prime limit меняет candidate lattice
адаптивного optimizer. В 5-limit C–E–G по-прежнему может стать 4:5:6, но
C–E–G–Bb больше не получает 7/4. В 11-limit optimizer может смешивать 3, 5,
7 и 11 в одном assignment без распознавания названия аккорда.

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
- Перед NoteOn нового голоса отправляется его конечный Pitch Bend. Каждое
  добавление/удаление активного голоса пересчитывает target всех остальных;
  изменившийся bend уже звучащих voices интерполируется без повторного NoteOn.
  Один scheduler обслуживает все glides без потоков на каждый voice и без
  пересчёта optimizer на каждом tick. При отсутствии glides он не просыпается
  периодически. Вывод отслеживается по voice ID, не по номеру канала.
- NoteOff использует назначенный канал. Если педаль отпущена, голос и канал
  освобождаются, bend возвращается в 8192. Если CC64 нажат, NoteOff всё равно
  отправляется синтезатору, но голос остаётся в строе, канал занят, а bend
  сохраняется до отпускания педали. NoteOn с velocity 0 равнозначен NoteOff;
  release velocity сохраняется.
- Повторные NoteOn одной ноты создают отдельные голоса. NoteOff снимает самый
  ранний ещё не отпущенный экземпляр (**FIFO отдельно для каждой пары input
  channel + note**). Это соглашение необходимо, поскольку MIDI 1.0 не передаёт ID.
- При 15 занятых каналах новый NoteOn игнорируется с warning; voice stealing нет.
  Его последующий NoteOff также учитывается в FIFO и не снимает более поздний голос.
- RPN 0 Pitch Bend Sensitivity (CC101/100, CC6/38, затем RPN Null) отправляется
  на channels 2..16 при старте и повторно перед выделенным голосом, чтобы
  настройку получили и подключившиеся позже клиенты. Включать MPE нужно в синтезаторе.
- CC64 (порог 64) и остальные CC транслируются на все member channels, включая
  свободные. При отпускании педали сначала отправляется CC64, затем удаляются
  только голоса с уже отпущенными клавишами, отменяются их glides, центрируются
  их bend и перенастраиваются оставшиеся голоса. Удерживаемые педалью ноты участвуют в
  адаптивном строе и могут получать новые плавные Pitch Bend. Повторное нажатие той же
  ноты создаёт отдельный голос на другом канале. CC120/123 работают как panic:
  сбрасываются голоса, sustain и bend.
- Входящие Pitch Bend, aftertouch, Program Change и системные/SysEx сообщения
  игнорируются с debug log. Входящие CC настройки диапазона могут изменить
  состояние синтезатора: во время игры не посылайте противоречащие настройкам RPN.

CC64 моделируется бинарно; half-pedal и sostenuto CC66 пока не поддерживаются.
Педаль может занять все 15 MPE-каналов: 16-й NoteOn будет отклонён без voice
stealing. После pedal-up физически удерживаемые клавиши продолжат звучать.

При Ctrl-C (также SIGTERM/SIGHUP) вход закрывается, отправляются NoteOff для
активных голосов, CC64=0 и CC123 на channels 1..16, затем центрирование bend
на channels 2..16. Та же очистка выполняется при ошибке отправки/переполнении
очереди. Все pending glides при этом отменяются. Даже после отдельной ошибки отправки программа пробует остальные
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
  glide.rs               monotonic per-voice glide и квантование обновлений
  voice.rs               Voice, FIFO экземпляров, allocator каналов
  pitch_bend.rs          чистая математика bend и RPN
  tuning/
    mod.rs               ActiveNote, TunedNote, TuningStrategy
    rational.rs          reduced Ratio, checked arithmetic, complexity
    candidates.rs        bounded arbitrary-prime search и кэш
    prime_limit.rs       PrimeLimit validation и effective prime set
    adaptive.rs          config, Costs, floating anchor, audible movement, beam search
    memory.rs            experimental relation fingerprints, decay and pair costs
    profile.rs           нормированные built-in/custom spectral profiles
    roughness.rs         абсолютные spectra и pairwise sensory roughness
    five_limit.rs        FiveLimitLowestRoot
examples/
  retune_benchmark.rs     диагностика времени пересчёта 15 voices
  glide_benchmark.rs      CPU-диагностика glide 15 voices без sleep/ALSA
  roughness_scan.rs       сравнение roughness кандидатов для двух MIDI notes
  ratio_scan.rs            кандидаты MIDI interval по prime limit
  memory_demo.rs           краткая диагностика relation fingerprint
  spectrum_dump.rs        нормированные partials выбранного профиля
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
Prime-limit покрыт тестами generic validation, фильтрации 5/7/11, новых 13/17/19
ratios, factor diagnostics, complexity-bounded search, candidate cache, top-K
pruning и выбора 11/8 optimizer-ом.
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
retrigger, sustain, glide/immediate A/B и очистку при SIGINT. По умолчанию этот тест
пропущен, чтобы `cargo test` работал без
`/dev/snd/seq`. Звучание в standalone/Ardour проверяется вручную по сценариям выше.

### Performance sanity check

```bash
cargo run --release --example retune_benchmark -- 100
cargo run --release --example glide_benchmark -- 10000
```

Без жёстких timing assertions. `retune_benchmark` измеряет 15 голосов при
sine, harmonic-default, bright и custom 32-partial профиле, а также cold
candidate generation для prime-limit 3/5/7/11/13/17/23/31/101. Стоимость
попарной roughness примерно растёт как `partials_A × partials_B`; большой
custom-профиль ожидаемо дороже.

Пример одного release-прогона (5 повторов, среднее для свежего аккорда):
sine ≈ 6.7 мс, harmonic ≈ 12.1 мс, bright ≈ 12.0 мс,
custom 32-partial ≈ 45.2 мс.

Измеряется только `tune()`, без создания optimizer, MIDI I/O и логирования.
Это ориентир конкретного запуска, а не гарантия задержки; параметры поиска,
нагрузка CPU и verbose output влияют на фактическое время.
`glide_benchmark` отдельно измеряет CPU-работу retarget/tick для 15 голосов
без ожидания реального времени и без запуска optimizer.
