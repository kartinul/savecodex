import { useRef, useState, type FC } from "react";
import { Flex, Heading, Text, Card, Button, Badge } from "@radix-ui/themes";
import { FileIcon, MagicWandIcon } from "@radix-ui/react-icons";

const Pack: FC = () => {
  const inputRef = useRef<HTMLInputElement>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const submit = async () => {
    const files = inputRef.current?.files;
    if (!files?.length) return;
    const form = new FormData();
    for (const f of Array.from(files)) form.append(f.webkitRelativePath || f.name, f);
    setLoading(true);
    try {
      const res = await fetch("/api/pack", { method: "POST", body: form });
      setStatus(JSON.stringify(await res.json(), null, 2));
    } catch (e) { setStatus(String(e)); }
    finally { setLoading(false); }
  };

  return (
    <Card size="3" style={{ width: 340 }}>
      <Flex direction="column" gap="3">
        <Flex align="center" gap="2">
          <FileIcon />
          <Heading size="4">Pack</Heading>
          <Badge color="gray" ml="auto">folder → DOCX + PDF</Badge>
        </Flex>
        <Text color="gray" size="2">Pick a folder — every code file gets run, output captured, and packaged.</Text>
        {/* @ts-expect-error webkitdirectory */}
        <input ref={inputRef} type="file" webkitdirectory="" multiple />
        <Button onClick={submit} disabled={loading}>{loading ? "Uploading…" : "Pack"}</Button>
        {status && <pre style={{ fontSize: 11, opacity: 0.6 }}>{status}</pre>}
      </Flex>
    </Card>
  );
};

const Solve: FC = () => {
  const inputRef = useRef<HTMLInputElement>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  const submit = async () => {
    const file = inputRef.current?.files?.[0];
    if (!file) return;
    const form = new FormData();
    form.append("assignment", file);
    setLoading(true);
    try {
      const res = await fetch("/api/solve", { method: "POST", body: form });
      setStatus(JSON.stringify(await res.json(), null, 2));
    } catch (e) { setStatus(String(e)); }
    finally { setLoading(false); }
  };

  return (
    <Card size="3" style={{ width: 340 }}>
      <Flex direction="column" gap="3">
        <Flex align="center" gap="2">
          <MagicWandIcon />
          <Heading size="4">Solve</Heading>
          <Badge color="violet" ml="auto">AI → DOCX + PDF</Badge>
        </Flex>
        <Text color="gray" size="2">Upload an assignment PDF or DOCX — AI writes the code, runs it, packages everything.</Text>
        <input ref={inputRef} type="file" accept=".pdf,.docx" />
        <Button onClick={submit} disabled={loading}>{loading ? "Uploading…" : "Solve"}</Button>
        {status && <pre style={{ fontSize: 11, opacity: 0.6 }}>{status}</pre>}
      </Flex>
    </Card>
  );
};

const App: FC = () => (
  <Flex direction="column" gap="6" p="6">
    <Heading size="6">SaveCodex</Heading>
    <Flex gap="4" wrap="wrap">
      <Pack />
      <Solve />
    </Flex>
  </Flex>
);

export default App;
