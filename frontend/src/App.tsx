import { useRef, useState, useEffect, type FC } from "react";
import { Flex, Heading, Text, Card, Button, Badge, TextField, Select, Switch, Grid, Box, IconButton, ScrollArea, Tooltip, Link, Callout } from "@radix-ui/themes";
import { FileIcon, GearIcon, LockClosedIcon, CheckIcon, Cross2Icon, InfoCircledIcon, QuestionMarkCircledIcon, GitHubLogoIcon } from "@radix-ui/react-icons";

function useLocalStorage<T>(key: string, initialValue: T): [T, (value: T) => void] {
  const [storedValue, setStoredValue] = useState<T>(() => {
    try {
      const item = window.localStorage.getItem(key);
      return item ? JSON.parse(item) : initialValue;
    } catch (error) {
      return initialValue;
    }
  });

  const setValue = (value: T) => {
    try {
      setStoredValue(value);
      window.localStorage.setItem(key, JSON.stringify(value));
    } catch (error) { }
  };

  return [storedValue, setValue];
}

const SUPPORTED_EXTENSIONS = ["java", "py", "rs", "js", "ts", "c", "cpp", "h", "hpp", "go", "rb", "php", "cs", "swift", "kt", "scala", "sh", "tsx", "jsx", "html", "css"];

const AdminKey: FC = () => {
  const [adminPassword, setAdminPassword] = useLocalStorage("savecodex_admin_pwd", "");
  const [isExpanded, setIsExpanded] = useState(false);
  const [status, setStatus] = useState<"idle" | "loading" | "success" | "error">("idle");
  const [isVerified, setIsVerified] = useState(false);
  const [hoverVerified, setHoverVerified] = useState(false);

  useEffect(() => {
    if (adminPassword) {
      fetch("/api/verify_admin", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ admin_password: adminPassword })
      })
        .then(res => res.json())
        .then(data => {
          if (data.success) setIsVerified(true);
        })
        .catch(() => { });
    }
  }, []);

  const handleSave = async () => {
    setStatus("loading");
    try {
      const res = await fetch("/api/verify_admin", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ admin_password: adminPassword })
      });

      if (res.ok) {
        const data = await res.json();
        if (data.success) {
          setStatus("success");
          setIsVerified(true);
          setTimeout(() => {
            setStatus("idle");
            setIsExpanded(false);
          }, 1000);
          return;
        }
      }

      setStatus("error");
      setTimeout(() => setStatus("idle"), 1000);
    } catch {
      setStatus("error");
      setTimeout(() => setStatus("idle"), 1000);
    }
  };

  const clearPassword = () => {
    setAdminPassword("");
    setIsVerified(false);
    setHoverVerified(false);
  };

  if (!isExpanded) {
    return (
      <Box style={{ position: "fixed", top: 16, right: 16, zIndex: 1000 }}>
        <Flex justify="end" style={{ padding: "4px" }}>
          {isVerified ? (
            <IconButton
              variant="ghost"
              color={hoverVerified ? "red" : "green"}
              onClick={hoverVerified ? clearPassword : () => { }}
              onMouseEnter={() => setHoverVerified(true)}
              onMouseLeave={() => setHoverVerified(false)}
              title={hoverVerified ? "Clear Admin Password" : "Admin Authenticated"}
              size="1"
            >
              {hoverVerified ? <Cross2Icon width="16" height="16" /> : <LockClosedIcon width="16" height="16" />}
            </IconButton>
          ) : (
            <Tooltip content="server admin access to bypass api key.">
              <IconButton
                variant="ghost"
                color="gray"
                size="1"
                onClick={() => setIsExpanded(true)}
              >
                <LockClosedIcon width="16" height="16" />
              </IconButton>
            </Tooltip>
          )}
        </Flex>
      </Box>
    );
  }

  return (
    <Box style={{ position: "fixed", top: 16, right: 16, zIndex: 1000 }}>
      <Flex
        align="center"
        gap="2"
        style={{
          background: "var(--color-panel)",
          padding: "4px",
          borderRadius: "var(--radius-3)",
          boxShadow: "var(--shadow-4)",
          border: status === "error" ? "1px solid var(--red-9)" : "1px solid transparent"
        }}
      >
        <TextField.Root
          type="password"
          placeholder="Admin Password"
          value={adminPassword}
          onChange={e => setAdminPassword(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter' && status !== "loading") handleSave();
          }}
          autoFocus
          disabled={status === "loading" || status === "success"}
          size="1"
          style={{ width: "120px" }}
        />
        <IconButton
          variant="soft"
          color={status === "error" ? "red" : status === "success" ? "green" : "blue"}
          onClick={handleSave}
          loading={status === "loading"}
          size="1"
        >
          <CheckIcon width="16" height="16" />
        </IconButton>
        <IconButton variant="ghost" color="gray" size="1" onClick={() => setIsExpanded(false)}>
          <Cross2Icon width="16" height="16" />
        </IconButton>
      </Flex>
    </Box>
  );
};


const Pack: FC = () => {
  const inputRef = useRef<HTMLInputElement>(null);
  const [loading, setLoading] = useState(false);
  const [folderName, setFolderName] = useState<string>("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string>("");

  // Keep track of the filtered list of files so user knows what will be packed
  const [detectedFiles, setDetectedFiles] = useState<string[]>([]);
  const [detectedExt, setDetectedExt] = useState<string>("");

  const [docTitleFocused, setDocTitleFocused] = useState(false);
  const [docTextFocused, setDocTextFocused] = useState(false);
  const [cwdFocused, setCwdFocused] = useState(false);

  const [ext, setExt] = useLocalStorage("savecodex_ext", "");
  const [docTitle, setDocTitle] = useLocalStorage("savecodex_doc_title", "");
  const [docText, setDocText] = useLocalStorage("savecodex_doc_text", "DOCX of: {}");
  const [fontSize, setFontSize] = useLocalStorage("savecodex_font_size", "18");
  const [theme, setTheme] = useLocalStorage("savecodex_theme", "dark");
  const [noPromptHighlight, setNoPromptHighlight] = useLocalStorage("savecodex_no_prompt", false);
  const [style, setStyle] = useLocalStorage("savecodex_style", "windows");
  const [username, setUsername] = useLocalStorage("savecodex_username", "admin");
  const [hostname, setHostname] = useLocalStorage("savecodex_hostname", "desktop");
  const [cwd, setCwd] = useLocalStorage("savecodex_cwd", "{}");
  const [pageBreak, setPageBreak] = useLocalStorage("savecodex_page_break", false);
  const [geminiApiKey, setGeminiApiKey] = useLocalStorage("savecodex_gemini_key", "");

  const [aiProvider, setAiProvider] = useLocalStorage("savecodex_ai_provider", "gemini");
  const [aiModel, setAiModel] = useLocalStorage("savecodex_ai_model", "gemini-3.5-flash-lite,gemini-3.1-flash-lite,gemini-2.5-flash-lite");
  const [aiBaseUrl, setAiBaseUrl] = useLocalStorage("savecodex_ai_base_url", "");

  const resetAdvanced = () => {
    setExt("");
    setDocTitle("{}");
    setDocText("DOCX of: {}");
    setFontSize("18");
    setTheme("dark");
    setNoPromptHighlight(false);
    setStyle("windows");
    setUsername("admin");
    setHostname("desktop");
    setCwd("{}");
    setPageBreak(false);
    setAiProvider("gemini");
    setAiModel("gemini-3.5-flash-lite,gemini-3.1-flash-lite,gemini-2.5-flash-lite");
    setAiBaseUrl("");
  };

  const onFileChange = () => {
    const files = inputRef.current?.files;
    if (files && files.length > 0) {
      const path = files[0].webkitRelativePath;
      if (path) {
        setFolderName(path.split("/")[0]);
      } else {
        setFolderName("files");
      }

      // Auto-detect extension logic
      const extCounts: Record<string, number> = {};
      const fileArray = Array.from(files);

      fileArray.forEach(f => {
        const parts = f.name.split(".");
        if (parts.length > 1) {
          const e = parts[parts.length - 1].toLowerCase();
          if (SUPPORTED_EXTENSIONS.includes(e)) {
            extCounts[e] = (extCounts[e] || 0) + 1;
          }
        }
      });

      let mostCommonExt = "";
      let maxCount = 0;
      for (const [e, count] of Object.entries(extCounts)) {
        if (count > maxCount) {
          maxCount = count;
          mostCommonExt = e;
        }
      }

      if (mostCommonExt) {
        setDetectedExt(mostCommonExt);

        // Filter UI view to show exactly what the backend would find by default
        const matching = fileArray
          .filter(f => f.name.endsWith("." + mostCommonExt))
          .map(f => f.webkitRelativePath);

        setDetectedFiles(matching);
      } else {
        setDetectedExt("");
        setDetectedFiles([]);
      }

    } else {
      setFolderName("");
      setDetectedFiles([]);
      setDetectedExt("");
    }
  };

  const submit = async () => {
    setErrorMessage("");

    const files = inputRef.current?.files;
    if (!files?.length) return;

    const form = new FormData();
    for (const f of Array.from(files)) {
      form.append(f.webkitRelativePath || f.name, f);
    }

    form.append("ext", ext);
    form.append("doc_title", docTitle || folderName);
    form.append("doc_text", docText);
    form.append("font_size", fontSize);
    form.append("theme", theme);
    form.append("no_prompt_highlight", String(noPromptHighlight));
    form.append("style", style);
    form.append("username", username);
    form.append("hostname", hostname);
    form.append("cwd", cwd || folderName || "~");
    form.append("page_break", String(pageBreak));
    form.append("gemini_api_key", geminiApiKey);
    try {
      const adminPwdStr = window.localStorage.getItem("savecodex_admin_pwd");
      form.append("admin_password", adminPwdStr ? JSON.parse(adminPwdStr) : "");
    } catch {
      form.append("admin_password", "");
    }
    form.append("ai_provider", aiProvider);
    form.append("ai_model", aiModel);
    form.append("ai_base_url", aiBaseUrl);

    setLoading(true);
    try {
      const res = await fetch("/api/pack", { method: "POST", body: form });
      if (!res.ok) {
        const err = await res.json().catch(() => ({ error: "Unknown error" }));
        setErrorMessage("Error: " + err.error);
        return;
      }

      const blob = await res.blob();
      const url = window.URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `${folderName || "pack"}_pack.docx`;
      document.body.appendChild(a);
      a.click();
      a.remove();
      window.URL.revokeObjectURL(url);

      // Clear files after successful download
      if (inputRef.current) {
        inputRef.current.value = "";
      }
      setFolderName("");
      setDetectedFiles([]);
      setDetectedExt("");

    } catch (e) {
      setErrorMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <Card size="3" style={{ width: 600, maxWidth: "100%", position: "relative" }}>
      <Flex direction="column" gap="4">
        <Flex align="center" gap="2">
          <FileIcon width="24" height="24" />
          <Heading size="5">Pack</Heading>
          <Badge color="blue" ml="auto">folder → DOCX</Badge>
          <IconButton
            variant="ghost"
            color="gray"
            onClick={() => setShowAdvanced(!showAdvanced)}
            title="Advanced Settings"
          >
            <GearIcon />
          </IconButton>
        </Flex>
        <Text color="gray" size="2" style={{ display: 'none' }}>Select a folder, Paste ur API key, and watch the DOCX get generated :)</Text>

        {errorMessage && (
          <Callout.Root color="red" size="1">
            <Callout.Icon>
              <InfoCircledIcon />
            </Callout.Icon>
            <Callout.Text>
              {errorMessage}
            </Callout.Text>
          </Callout.Root>
        )}

        <Box
          style={{
            border: "2px dashed var(--gray-6)",
            borderRadius: "var(--radius-3)",
            padding: "var(--space-4)",
            textAlign: "center",
            position: "relative",
            cursor: "pointer",
            background: "var(--gray-2)"
          }}
        >
          <Tooltip content="Files are processed in alphabetical order. Prefix filenames with numbers (e.g. 1_main.py, 2_utils.py) to control the exact execution order.">
            <Box style={{ position: "absolute", top: 8, right: 8, zIndex: 10 }}>
              <QuestionMarkCircledIcon width="16" height="16" style={{ color: "var(--gray-9)" }} />
            </Box>
          </Tooltip>
          <input
            ref={inputRef}
            type="file"
            {...{ webkitdirectory: "" } as any}
            multiple
            onChange={onFileChange}
            style={{
              position: "absolute",
              inset: 0,
              opacity: 0,
              cursor: "pointer"
            }}
          />
          <Text size="3" weight="bold" color="blue">
            {folderName ? `Selected: ${folderName}` : "Click to select a folder"}
          </Text>
        </Box>

        {detectedFiles.length > 0 && (
          <Box mt="2">
            <Flex align="center" gap="2" mb="2">
              <Text size="2" weight="bold">Files to process</Text>
              {detectedExt && <Badge color="green">Auto-detected: .{detectedExt}</Badge>}
            </Flex>
            <ScrollArea type="always" scrollbars="vertical" style={{ height: 120, border: "1px solid var(--gray-5)", borderRadius: "var(--radius-2)", padding: "var(--space-2)" }}>
              {detectedFiles.map((file, i) => (
                <Text as="div" size="1" color="gray" key={i}>{file}</Text>
              ))}
            </ScrollArea>
          </Box>
        )}

        <Grid columns="2" gap="3">
          <Box>
            <Text as="div" size="2" mb="1" weight="bold">Document Title</Text>
            <TextField.Root
              placeholder={folderName || "Default (Folder Name)"}
              value={docTitleFocused ? docTitle : docTitle.replace("{}", folderName || "folder")}
              onChange={e => setDocTitle(e.target.value)}
              onFocus={() => setDocTitleFocused(true)}
              onBlur={() => setDocTitleFocused(false)}
            />
          </Box>
          <Box>
            <Flex align="center" gap="2" mb="1">
              <Text as="div" size="2" weight="bold">GEMINI API KEY</Text>
              <Link size="1" href="https://aistudio.google.com/app/apikey" target="_blank">(Get Key)</Link>
              <Tooltip content="AI analyzes your code to figure out what inputs it asks for, so it can run the program automatically for you without needing you to manually type anything.">
                <InfoCircledIcon width="14" height="14" style={{ color: "var(--gray-9)" }} />
              </Tooltip>
            </Flex>
            <TextField.Root
              type="password"
              placeholder="Your API Key"
              value={geminiApiKey}
              onChange={e => setGeminiApiKey(e.target.value)}
            />
          </Box>
          <Box>
            <Text as="div" size="2" mb="1" weight="bold">Document Text</Text>
            <TextField.Root
              value={docTextFocused ? docText : docText.replace("{}", folderName || "folder")}
              onChange={e => setDocText(e.target.value)}
              onFocus={() => setDocTextFocused(true)}
              onBlur={() => setDocTextFocused(false)}
            />
          </Box>
          <Box>
            <Text as="div" size="2" mb="1" weight="bold">Username</Text>
            <TextField.Root value={username} onChange={e => setUsername(e.target.value)} />
          </Box>
        </Grid>

        {showAdvanced && (
          <Flex direction="column" gap="3" mt="2" p="3" style={{ background: "var(--gray-2)", borderRadius: "var(--radius-3)" }}>
            <Flex align="center" justify="between">
              <Text size="3" weight="bold">Advanced Settings</Text>
              <Button size="1" variant="soft" color="gray" onClick={resetAdvanced} type="button">Reset Defaults</Button>
            </Flex>
            <Grid columns="2" gap="3">
              <Box>
                <Text as="div" size="2" mb="1">Extensions Override (CSV)</Text>
                <TextField.Root value={ext} onChange={e => setExt(e.target.value)} />
              </Box>
              <Box>
                <Text as="div" size="2" mb="1">AI Provider</Text>
                <Select.Root value={aiProvider} onValueChange={setAiProvider}>
                  <Select.Trigger style={{ width: "100%" }} />
                  <Select.Content>
                    <Select.Item value="gemini">Gemini</Select.Item>
                    <Select.Item value="openai_compatible">OpenAI Compatible</Select.Item>
                  </Select.Content>
                </Select.Root>
              </Box>
              <Box>
                <Text as="div" size="2" mb="1">AI Model(s)</Text>
                <TextField.Root
                  placeholder="e.g. gemini-1.5-pro"
                  value={aiModel}
                  onChange={e => setAiModel(e.target.value)}
                />
              </Box>
              {aiProvider === "openai_compatible" && (
                <Box style={{ gridColumn: "span 2" }}>
                  <Text as="div" size="2" mb="1">Base URL</Text>
                  <TextField.Root
                    placeholder="https://api.groq.com/openai/v1"
                    value={aiBaseUrl}
                    onChange={e => setAiBaseUrl(e.target.value)}
                  />
                </Box>
              )}
              <Box>
                <Text as="div" size="2" mb="1">Font Size</Text>
                <TextField.Root type="number" value={fontSize} onChange={e => setFontSize(e.target.value)} />
              </Box>

              <Box>
                <Text as="div" size="2" mb="1">Theme</Text>
                <Select.Root value={theme} onValueChange={setTheme}>
                  <Select.Trigger style={{ width: "100%" }} />
                  <Select.Content>
                    <Select.Item value="dark">Dark</Select.Item>
                    <Select.Item value="light">Light</Select.Item>
                  </Select.Content>
                </Select.Root>
              </Box>
              <Box>
                <Text as="div" size="2" mb="1">Window Style</Text>
                <Select.Root value={style} onValueChange={setStyle}>
                  <Select.Trigger style={{ width: "100%" }} />
                  <Select.Content>
                    <Select.Item value="windows">Windows</Select.Item>
                    <Select.Item value="macos">macOS</Select.Item>
                    <Select.Item value="linux">Linux</Select.Item>
                  </Select.Content>
                </Select.Root>
              </Box>

              <Box>
                <Text as="div" size="2" mb="1">Hostname</Text>
                <TextField.Root value={hostname} onChange={e => setHostname(e.target.value)} />
              </Box>
              <Box>
                <Text as="div" size="2" mb="1">CWD</Text>
                <TextField.Root
                  value={cwdFocused ? cwd : cwd.replace("{}", folderName || "folder")}
                  onChange={e => setCwd(e.target.value)}
                  onFocus={() => setCwdFocused(true)}
                  onBlur={() => setCwdFocused(false)}
                />
              </Box>
              <Flex align="center" gap="2" mt="2">
                <Switch checked={noPromptHighlight} onCheckedChange={setNoPromptHighlight} />
                <Text size="2">No Prompt Highlight</Text>
              </Flex>
              <Flex align="center" gap="2" mt="2">
                <Switch checked={pageBreak} onCheckedChange={setPageBreak} />
                <Text size="2">Page Break per File</Text>
              </Flex>
            </Grid>
          </Flex>
        )}

        <Button size="3" mt="2" onClick={submit} disabled={loading || !folderName}>
          {loading ? "Packing..." : "Pack"}
        </Button>
      </Flex>
    </Card>
  );
};

const App: FC = () => (
  <Flex direction="column" align="center" justify="center" gap="6" p="6" style={{ minHeight: "100vh" }}>
    <AdminKey />
    <Flex direction="column" align="center" gap="2" mb="4" style={{ textAlign: "center", maxWidth: 600 }}>
      <Heading size="8" color="blue">
        savecodex
      </Heading>
      <Text color="gray" size="3">
        savecodex reads all the files in the folder you select, generate input automatically using ai, types those inputs, takes screenshots, makes documents and downloads it in docx.
      </Text>
      <Text size="2" style={{ color: "var(--gray-10)", opacity: 0.6 }}>
        (note: some labs require you to manually "save as" and save the doc in a seperate place before uploading)
      </Text>
    </Flex>
    <Pack />

    <Flex align="center" gap="2" style={{ marginTop: "auto" }}>
      <Text size="2" color="gray">
        Made by <a href="https://github.com/kartinul" target="_blank" rel="noopener noreferrer" style={{ color: "inherit", textDecoration: "none" }}>kartinul</a>
      </Text>
      <a href="https://github.com/kartinul" target="_blank" rel="noopener noreferrer" style={{ color: "var(--gray-11)", display: "flex", alignItems: "center" }}>
        <GitHubLogoIcon width="16" height="16" />
      </a>
    </Flex>
  </Flex>
);

export default App;
