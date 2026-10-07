<?php declare(strict_types=1);

namespace Typedhp;

use type Typedhp\Result;

final readonly class ProcessRun
{
    public function __construct(public int $exitCode, public string $output, public string $errors) {}

    public static function start(NonEmptyList<string> $command, string $input): Result<self, NonEmptyString> {
        $pipes = [];
        $process = proc_open($command, [['pipe', 'r'], ['pipe', 'w'], ['pipe', 'w']], $pipes);
        $stdin = $pipes[0] ?? null;
        $stdout = $pipes[1] ?? null;
        $stderr = $pipes[2] ?? null;
        $isStarted = $process !== false && $stdin !== null && $stdout !== null && $stderr !== null;
        if (!$isStarted) {
            return new Err("typedhp: cannot run {$command[0]}");
        }

        fwrite($stdin, $input);
        fclose($stdin);
        $output = stream_get_contents($stdout);
        $errors = stream_get_contents($stderr);
        fclose($stdout);
        fclose($stderr);
        $exitCode = proc_close($process);
        $isRead = $output !== false && $errors !== false;
        if (!$isRead) {
            return new Err("typedhp: cannot read the output of {$command[0]}");
        }

        $run = new self($exitCode, $output, $errors);
        return new Ok($run);
    }
}
