<p align="center">
  <img src="assets/aysh-logo.png" alt="aysh" width="200">
</p>

<h1 align="center">aysh</h1>

<p align="center">
  Лёгкая командная оболочка на Rust. Без сети, без телеметрии, без сторонних тяжёлых зависимостей.
</p>

---

## Возможности

- Запуск любых программ из `PATH`
- Конвейеры `|` и перенаправления `<`, `>`, `>>`
- Кавычки, экранирование `\` и подстановка переменных: `$NAME`, `${NAME}`, `$?`
- Встроенные команды: `cd`, `pwd`, `echo`, `export`, `unset`, `help`, `exit`
- Настройка через TOML: приглашение, автозапуск команд и оформление текста
- Цветное и жирное/курсивное оформление через ANSI (`black`, `red`, `cyan` / `голубой`)

## Установка и запуск

Нужен установленный [Rust](https://www.rust-lang.org/tools/install).

### Быстрый запуск без установки

```sh
git clone https://github.com/aydevix/aysh.git
cd aysh
cargo run --release
```

### Полная установка в систему

Вариант 1 — в `~/.cargo/bin` (обычно уже есть в `PATH`):

```sh
cargo install --git https://github.com/aydevix/aysh
```

Вариант 2 — системно, для всех пользователей (нужны права администратора):

```sh
git clone https://github.com/aydevix/aysh.git
cd aysh
cargo build --release
sudo install -m 755 target/release/aysh /usr/local/bin/aysh
```

Проверка:

```sh
which aysh
aysh
```

### Использовать как основную оболочку (необязательно)

Чтобы `aysh` можно было выбрать в качестве оболочки входа, добавьте его путь в `/etc/shells` и смените оболочку:

```sh
echo /usr/local/bin/aysh | sudo tee -a /etc/shells
chsh -s /usr/local/bin/aysh
```

> `aysh` не читает `~/.profile`, `~/.bashrc` и подобные файлы. Для входа в систему используйте его осторожно и держите под рукой другой терминал или загрузочную оболочку, чтобы в случае проблем можно было откатить `chsh`.

### Удаление

```sh
cargo uninstall aysh                 # если ставили через cargo install
sudo rm /usr/local/bin/aysh          # если ставили вручную
rm -rf ~/.config/aysh                # необязательно: удалить конфигурацию
```

## Конфигурация

Файл настроек: `$XDG_CONFIG_HOME/aysh/config.toml`, а если `XDG_CONFIG_HOME` не задан, то `~/.config/aysh/config.toml`.

```toml
prompt = "aysh> "
autostart = ["echo Добро пожаловать в aysh", "pwd"]

[style]
bold = true
italic = false
color = "cyan"   # black, red или cyan (голубой)
```

| Ключ | Тип | Описание |
| --- | --- | --- |
| `prompt` | строка | Приглашение. По умолчанию `# ` |
| `autostart` | строка или массив | Команды, которые выполняются при запуске |
| `style.bold` | `true` / `false` | Жирный текст для приглашения и вывода `echo` |
| `style.italic` | `true` / `false` | Курсив |
| `style.color` | `black`, `red`, `cyan` | Цвет текста |

Ошибки в конфиге выводятся в stderr, после чего оболочка запускается с настройками по умолчанию.

## Форматирование через `echo`

```sh
echo --bold --color red Ошибка!
echo --italic --color=cyan Текст курсивом
```

Оформление применяется только при выводе в терминал. При перенаправлении в файл или канал ANSI-коды не записываются.

## Пример

```text
# echo --bold привет
привет
# cd /tmp && pwd
/tmp
# ls | grep txt > list.txt
# exit
```

## Ограничения

`aysh` намеренно компактен и не является полной реализацией POSIX shell. Не поддерживаются подстановка команд, globbing, фоновые задачи, условные конструкции и функции.

## Память и приватность

В оболочке нет телеметрии, сетевых запросов и фоновых служб. Потребление памяти зависит от ОС и окружения; память запускаемых программ в него не входит.

## Разработка

```sh
cargo test
cargo fmt --check
```

## Лицензия

MIT
