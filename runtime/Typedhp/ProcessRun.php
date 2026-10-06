<?php declare(strict_types=1);

namespace Typedhp;

use RuntimeException;

final readonly class ProcessRun
{
    public function __construct(public int $exitCode, public string $output, public string $errors) {}

    /** @param non-empty-list<string> $command */
    public static function start(array $command, string $input): self {
        $pipes = [];
        $process = proc_open($command, [['pipe', 'r'], ['pipe', 'w'], ['pipe', 'w']], $pipes);
        $stdin = $pipes[0] ?? null;
        $stdout = $pipes[1] ?? null;
        $stderr = $pipes[2] ?? null;
        if ($process === false || $stdin === null || $stdout === null || $stderr === null) {
            throw new RuntimeException("typedhp: cannot run {$command[0]}");
        }

        fwrite($stdin, $input);
        fclose($stdin);
        $output = stream_get_contents($stdout);
        $errors = stream_get_contents($stderr);
        fclose($stdout);
        fclose($stderr);
        $exitCode = proc_close($process);
        if ($output === false || $errors === false) {
            throw new RuntimeException("typedhp: cannot read the output of {$command[0]}");
        }

        return new self($exitCode, $output, $errors);
    }
}
