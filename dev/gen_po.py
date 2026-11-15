#!/usr/bin/env python3
"""Writes crates/app/translations/<lang>/LC_MESSAGES/fastclock.po (bundled by build.rs). Edit the table, run, commit.
Slint's default gettext context is the component name, so msgctxt is `Main`, `AlarmPage`, `TimerPage`, `StopwatchPage`,
`PomodoroPage`, `StartPauseReset`, `AlarmDialog`, `RingingDialog`, `SettingsDialog`, `About` (UI) or `Tr` (strings built in Rust)."""
import os

OUT = os.path.join(os.path.dirname(__file__), "..", "crates", "app", "translations")
LANGS = ["pt_BR"]
# (context, msgid, [translation per LANGS])
T = [
    # Built in Rust (the `Tr` global in main.slint).
    ("Tr", "Mon", ["Seg"]),
    ("Tr", "Tue", ["Ter"]),
    ("Tr", "Wed", ["Qua"]),
    ("Tr", "Thu", ["Qui"]),
    ("Tr", "Fri", ["Sex"]),
    ("Tr", "Sat", ["Sáb"]),
    ("Tr", "Sun", ["Dom"]),
    ("Tr", "Once", ["Uma vez"]),
    ("Tr", "Every day", ["Todos os dias"]),
    ("Tr", "Weekdays", ["Dias úteis"]),
    ("Tr", "Alarm", ["Alarme"]),
    ("Tr", "Time's up", ["O tempo acabou"]),
    ("Tr", "Work", ["Foco"]),
    ("Tr", "Break", ["Pausa"]),
    ("Tr", "Long break", ["Pausa longa"]),
    ("Tr", "Round {} of {}", ["Rodada {} de {}"]),
    ("Tr", "Work session finished", ["Sessão de foco concluída"]),
    ("Tr", "Break is over", ["A pausa acabou"]),
    ("Tr", "Next alarm in {} min", ["Próximo alarme em {} min"]),
    ("Tr", "Next alarm in {} h {} min", ["Próximo alarme em {} h {} min"]),
    ("Tr", "Next alarm in {} d {} h", ["Próximo alarme em {} d {} h"]),
    ("Tr", "Snooze", ["Soneca"]),
    ("Tr", "Stop", ["Parar"]),
    ("Tr", "Choose the alarm sound", ["Escolha o som do alarme"]),
    ("Tr", "Sound files", ["Arquivos de som"]),
    ("Tr", "Open FastClock", ["Abrir o FastClock"]),
    ("Tr", "Quit", ["Sair"]),
    # The window.
    ("Main", "Preferences", ["Preferências"]),
    ("Main", "Alarm", ["Alarme"]),
    ("Main", "Timer", ["Timer"]),
    ("Main", "Stopwatch", ["Cronômetro"]),
    ("Main", "Pomodoro", ["Pomodoro"]),
    ("AlarmPage", "Alarm", ["Alarme"]),
    ("AlarmPage", "New alarm", ["Novo alarme"]),
    ("AlarmPage", "Delete", ["Excluir"]),
    ("AlarmPage", "More", ["Mais"]),
    ("AlarmPage", "No alarms yet. Press New alarm.", ["Nenhum alarme ainda. Clique em Novo alarme."]),
    ("TimerPage", "Hours", ["Horas"]),
    ("TimerPage", "Minutes", ["Minutos"]),
    ("TimerPage", "Seconds", ["Segundos"]),
    ("PomodoroPage", "Skip", ["Pular"]),
    ("StartPauseReset", "Pause", ["Pausar"]),
    ("StartPauseReset", "Resume", ["Continuar"]),
    ("StartPauseReset", "Start", ["Iniciar"]),
    ("StartPauseReset", "Reset", ["Zerar"]),
    ("AlarmDialog", "New alarm", ["Novo alarme"]),
    ("AlarmDialog", "Edit alarm", ["Editar alarme"]),
    ("AlarmDialog", "hour", ["hora"]),
    ("AlarmDialog", "minute", ["minuto"]),
    ("TimeStepper", "Increase {}", ["Aumentar {}"]),
    ("TimeStepper", "Decrease {}", ["Diminuir {}"]),
    ("TimerPage", "{} min", ["{} min"]),
    ("AlarmDialog", "Label (optional)", ["Nome (opcional)"]),
    ("AlarmDialog", "Pick no day to ring once.", ["Sem nenhum dia marcado, toca uma vez."]),
    ("AlarmDialog", "Delete", ["Excluir"]),
    ("AlarmDialog", "Weekdays", ["Dias úteis"]),
    ("AlarmDialog", "Weekend", ["Fim de semana"]),
    ("AlarmDialog", "Cancel", ["Cancelar"]),
    ("AlarmDialog", "Save", ["Salvar"]),
    ("RingingDialog", "Snooze", ["Soneca"]),
    ("RingingDialog", "Stop", ["Parar"]),
    ("SettingsDialog", "Preferences", ["Preferências"]),
    ("SettingsDialog", "Alarm sound", ["Som do alarme"]),
    ("SettingsDialog", "Bell", ["Sino"]),
    ("SettingsDialog", "Reset", ["Restaurar"]),
    ("SettingsDialog", "Test", ["Testar"]),
    ("SettingsDialog", "More", ["Mais"]),
    ("SettingsDialog", "Choose…", ["Escolher…"]),
    ("SettingsDialog", "Snooze (minutes)", ["Soneca (minutos)"]),
    ("SettingsDialog", "Start at login", ["Iniciar com o sistema"]),
    ("SettingsDialog", "Appearance", ["Aparência"]),
    ("SettingsDialog", "System", ["Sistema"]),
    ("SettingsDialog", "Light", ["Claro"]),
    ("SettingsDialog", "Dark", ["Escuro"]),
    ("SettingsDialog", "Language", ["Idioma"]),
    ("SettingsDialog", "About", ["Sobre"]),
    ("SettingsDialog", "Close", ["Fechar"]),
    ("About", "A light and fast alarm, timer, stopwatch and Pomodoro for Linux.", ["Alarme, temporizador, cronômetro e Pomodoro leves e rápidos para Linux."]),
    # The About card of the Fast family kit (about.slint): the same strings, in the same context, in every app.
    ("AboutCard", "Version {}", ["Versão {}"]),
    ("AboutCard", "Author: {}", ["Autor: {}"]),
    ("AboutCard", "License: {}", ["Licença: {}"]),
    ("AboutCard", "Close", ["Fechar"]),
]

q = lambda s: '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'
for i, lang in enumerate(LANGS):
    d = f"{OUT}/{lang}/LC_MESSAGES"
    os.makedirs(d, exist_ok=True)
    with open(f"{d}/fastclock.po", "w") as f:
        f.write(f'msgid ""\nmsgstr ""\n"Content-Type: text/plain; charset=UTF-8\\n"\n"Language: {lang}\\n"\n')
        for ctx, msgid, tr in T:
            assert len(tr) == len(LANGS) and tr[i].count("{}") == msgid.count("{}"), msgid
            f.write(f"\nmsgctxt {q(ctx)}\nmsgid {q(msgid)}\nmsgstr {q(tr[i])}\n")
