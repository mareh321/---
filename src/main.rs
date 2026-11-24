use list_rs::{number, plot, read_xml, sheet::Sheet, Calc, DEMO};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const HELP: &str = "list-rs [путь.xmcd или путь.xmcdz] [--once | --check]\n\n\
    list               Показать значения и сообщения импорта\n\
    show НОМЕР         Показать все элементы выбранной матрицы\n\
    plot X Y ФАЙЛ      Сохранить график двух векторов или диапазонов в HTML\n\
    plotc Z ФАЙЛ       Сохранить комплексный вектор на плоскости Re–Im\n\
    set НОМЕР ЧИСЛО    Изменить вход (*) в его исходных единицах\n\
    set НОМЕР ЧИСЛО ЕД Изменить вход с явной единицей, например kN\n\
    reset НОМЕР        Вернуть исходное значение\n\
    bench НОМЕР A B    Сравнить два способа пересчёта при значениях A и B\n\
    help               Показать команды\n\
    quit               Выйти";

fn list(sheet: &Sheet) -> usize {
    let mut issues = 0;
    for (index, region) in sheet.regions.iter().enumerate() {
        if region.imported {
            continue;
        }
        let number = index + 1;
        let source = &region.source_id;
        if let Some(text) = &region.text {
            println!("  #{number:<3} [{source}] {text}");
            continue;
        }
        let editable = if sheet.editable(index) { '*' } else { ' ' };
        let label = &region.label;
        match sheet.display(index) {
            Ok(result) => println!("{editable} #{number:<3} [{source}] {label} = {result}"),
            Err(reason) => {
                issues += 1;
                println!("! #{number:<3} [{source}] {label} — {reason}");
            }
        }
    }
    if issues > 0 {
        println!("Регионов с ошибкой или неподдерживаемой формулой: {issues}");
    }
    issues
}

fn region_number(token: &str) -> Calc<usize> {
    token
        .parse::<usize>()
        .ok()
        .and_then(|number| number.checked_sub(1))
        .ok_or_else(|| "укажите номер региона, начиная с 1".into())
}

fn benchmark(xml: &str, index: usize, a: f64, b: f64) -> Calc<()> {
    if a == b {
        return Err("для сравнения нужны два разных значения".into());
    }
    let mut cached = Sheet::import(xml)?;
    let mut full = Sheet::import(xml)?;
    cached.full();
    full.full();

    for value in [a, b] {
        cached.set(index, Some(value), None)?;
        cached.incremental();
        full.set_and_recalculate_all(index, value)?;
        if let Some(region) = cached.first_difference(&full) {
            return Err(format!("расхождение в регионе #{}", region + 1));
        }
    }

    let mut cached_elapsed = Duration::ZERO;
    let mut full_elapsed = Duration::ZERO;
    let mut cached_count = 0;
    let mut full_count = 0;
    let iterations = 300;
    for step in 0..iterations {
        let value = if step % 2 == 0 { a } else { b };
        let start = Instant::now();
        cached.set(index, Some(value), None)?;
        cached_count = cached.incremental();
        cached_elapsed += start.elapsed();
        let start = Instant::now();
        full_count = full.set_and_recalculate_all(index, value)?;
        full_elapsed += start.elapsed();
    }
    println!("Идентичные результаты обоих режимов для двух входных значений.");
    println!("Зависимый пересчёт: {cached_count} из {full_count} формул за одно изменение.");
    println!("Среднее за {iterations} изменений входа с пересчётом, без импорта и печати:");
    println!(
        "  зависимый {:>11.3} мкс",
        cached_elapsed.as_secs_f64() * 1e6 / f64::from(iterations)
    );
    println!(
        "  полный    {:>11.3} мкс",
        full_elapsed.as_secs_f64() * 1e6 / f64::from(iterations)
    );
    Ok(())
}

fn process(sheet: &mut Sheet, xml: &str, command: &str) -> Calc<bool> {
    let tokens: Vec<_> = command.split_whitespace().collect();
    match tokens.as_slice() {
        [] => {}
        ["list"] => {
            list(sheet);
        }
        ["help"] => println!("{HELP}"),
        ["show", index] => println!("{}", sheet.details(region_number(index)?)?),
        ["plot", x, y, path @ ..] if !path.is_empty() => {
            let path = path.join(" ");
            plot::write(sheet, x, Some(y), Path::new(path.trim_matches('"')))?;
            println!("График сохранён в {path}");
        }
        ["plotc", series, path @ ..] if !path.is_empty() => {
            let path = path.join(" ");
            plot::write(sheet, series, None, Path::new(path.trim_matches('"')))?;
            println!("График сохранён в {path}");
        }
        ["quit" | "exit"] => return Ok(false),
        ["bench", index, a, b] => benchmark(xml, region_number(index)?, number(a)?, number(b)?)?,
        ["set", index, value, unit @ ..] if unit.len() <= 1 => {
            sheet.set(
                region_number(index)?,
                Some(number(value)?),
                unit.first().copied(),
            )?;
            println!("Пересчитано формул: {}", sheet.incremental());
            list(sheet);
        }
        ["reset", index] => {
            sheet.set(region_number(index)?, None, None)?;
            println!("Пересчитано формул: {}", sheet.incremental());
            list(sheet);
        }
        _ => return Err("неизвестная команда или неверные аргументы. Введите help".into()),
    }
    Ok(true)
}

fn run() -> Calc<bool> {
    let mut path = None;
    let mut once = false;
    let mut check = false;
    for arg in std::env::args_os().skip(1) {
        if arg == "--help" || arg == "-h" {
            println!("{HELP}");
            return Ok(true);
        }
        if arg == "--once" {
            once = true;
        } else if arg == "--check" {
            check = true;
            once = true;
        } else if path.is_none() {
            path = Some(arg);
        } else {
            return Err("укажите один путь к листу и, при необходимости, --once".into());
        }
    }
    let title = path.as_deref().map(|path| path.to_string_lossy());
    let xml = if let Some(path) = &path {
        read_xml(Path::new(path))?
    } else {
        DEMO.into()
    };
    let start = Instant::now();
    let base_dir = path.as_deref().and_then(|p| Path::new(p).parent());
    let mut sheet = Sheet::import_with_base(&xml, base_dir)?;
    let import_time = start.elapsed();
    let start = Instant::now();
    let count = sheet.full();
    let calc_time = start.elapsed();
    println!(
        "Лист · {}",
        title.as_deref().unwrap_or("Давление под опорой (демо)")
    );
    println!(
        "Импорт {:.3} мс. Полный расчёт {:.3} мс ({count} формул).",
        import_time.as_secs_f64() * 1e3,
        calc_time.as_secs_f64() * 1e3
    );
    let issues = list(&sheet);
    let verified = if check {
        sheet.verify_snapshots()
    } else {
        true
    };
    if once {
        return Ok(issues == 0 && verified);
    }
    println!(
        "* — изменяемый вход. Команды: help, list, show, set, reset, plot, plotc, bench, quit."
    );
    let mut stdin = io::stdin().lock();
    let mut command = String::new();
    loop {
        print!("> ");
        io::stdout().flush().map_err(|error| error.to_string())?;
        command.clear();
        if stdin
            .read_line(&mut command)
            .map_err(|error| error.to_string())?
            == 0
        {
            break;
        }
        match process(&mut sheet, &xml, &command) {
            Ok(true) => {}
            Ok(false) => break,
            Err(reason) => eprintln!("Ошибка: {reason}"),
        }
    }
    Ok(true)
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(2),
        Err(reason) => {
            eprintln!("Ошибка: {reason}");
            ExitCode::FAILURE
        }
    }
}
