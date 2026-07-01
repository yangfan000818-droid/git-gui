<script lang="ts">
  export let name: string = "";
  export let isDir: boolean = false;
  export let isOpen: boolean = false;

  $: ext = name.includes(".") ? name.split(".").pop()?.toLowerCase() : "";

  // Svelte Icon
  const svgSvelte = `<svg viewBox="0 0 32 32" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M14.542 3.655C15.424 3 16.576 3 17.458 3.655l8.777 6.51c1.238.92 1.63 2.656.883 4.02l-1.077 1.966a3 3 3 0 0 1-4.086 1.258l-5.632-2.923c-1-.52-2.181 0-2.316 1.135l-.176 1.48c-.144 1.21.737 2.274 1.954 2.368l8.243.636a3.298 3.298 0 0 1 3.036 3.284v1.892c0 1.25-.8 2.37-2 2.766l-5.833 1.916a2.91 2.91 0 0 1-1.63.023l-7.904-2.146c-1.378-.374-2.336-1.583-2.383-3.003L7.26 18.064c-.035-1.026.711-1.933 1.733-2.1l7.108-1.156c1.077-.175 1.701-1.34 1.237-2.313l-.53-1.111a1.951 1.951 0 0 0-2.73-.854L8.71 13.567c-1.25.717-2.825.266-3.52-1.008l-1.03-1.884c-.746-1.365-.354-3.1 .884-4.02l9.498-7Z" fill="#ff3e00"/></svg>`;
  // TypeScript Icon
  const svgTs = `<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg"><path fill="#3178c6" d="M3.5 3h25v25h-25z"/><path fill="#fff" d="M17 14h5.5v2.2h-2.1v9.3h-2.3v-9.3h-2.1V14zm11.3 7c-.2-1.9-1.5-2.7-3.2-3.1-1.2-.3-1.6-.6-1.6-1.1 0-.6.6-1 1.6-1s1.7.5 1.8 1.4h2.2c-.2-1.9-1.6-3.2-3.8-3.2-2.3 0-4 1.2-4 3 0 1.8 1.5 2.6 3.3 3 .9.2 1.4.6 1.4 1.2 0 .6-.7 1.1-1.8 1.1-1.2 0-2-.5-2.3-1.6h-2.2c.2 2 1.9 3.4 4.3 3.4 2.5 0 4.1-1.2 4.1-3.1z"/></svg>`;
  // JavaScript Icon
  const svgJs = `<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg"><path fill="#f7df1e" d="M3 3h26v26H3z"/><path fill="#000" d="M16 23.9c0 .7-.1 1.2-.4 1.5-.3.4-.8.5-1.5.5-1 0-1.6-.4-1.9-1.3h-3.4c.3 1.5 1.1 2.6 2 3.3.9.7 2.1 1 3.4 1 1.6 0 2.8-.5 3.7-1.3.9-.9 1.3-2 1.3-3.6V15h-3.2v8.9zm13-.5c-.2 1.1-.7 2-1.6 2.7-.9.6-2 .9-3.4.9-1.4 0-2.6-.4-3.5-1.1-.9-.7-1.5-1.7-1.7-2.9h3.3c.1.6.4 1.1.9 1.5.5.3 1 .5 1.7.5 1.1 0 1.9-.3 2.4-.8.5-.6.8-1.3.8-2.2 0-1-.3-1.7-.9-2.2-.6-.5-1.6-.9-3.2-1.4-1.7-.4-2.8-.9-3.5-1.5-.6-.5-1.1-1.2-1.4-1.9-.2-.7-.4-1.6-.4-2.7 0-1.4.5-2.5 1.3-3.4 1-.9 2.2-1.3 3.8-1.3 1.4 0 2.5.4 3.4 1s1.4 1.5 1.6 2.6h-3.2c-.1-.5-.3-.9-.7-1.2-.4-.3-1-.5-1.6-.5-.9 0-1.5.2-2 .7-.5.4-.7 1.1-.7 2 0 .7.2 1.3.5 1.7.4.4 1.3.8 2.8 1.2 1.8.5 3.1 1.1 3.8 1.7.7.7 1.1 1.4 1.4 2.3.2.7.3 1.5.3 2.6z"/></svg>`;
  // JSON Icon
  const svgJson = `<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg"><path fill="#fbc02d" d="M12.9 8.2h6.2v15.6h-6.2z"/><path fill="#fff" d="M11 11.2v9.6h-2v-9.6h2zm10 0v9.6h2v-9.6h-2z"/></svg>`;
  // Rust Icon
  const svgRust = `<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg"><path fill="#dea584" d="M16 2.8c-7.3 0-13.2 5.9-13.2 13.2S8.7 29.2 16 29.2 29.2 23.3 29.2 16 23.3 2.8 16 2.8z"/><path fill="#fff" d="M21.2 18.7c.9-1.2 1.3-2.6 1.3-4.2 0-3.3-2.4-5.5-5.9-5.5H10v14.1h3.3v-4h1.7l3 4h4.1l-3.5-4.4h.6c.7 0 1.4-.2 2-.7zm-7.9-6.9h2.6c1.6 0 2.6.9 2.6 2.4s-1 2.4-2.6 2.4h-2.6v-4.8z"/></svg>`;
  // Markdown Icon
  const svgMd = `<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg"><path fill="#519aba" d="M3 6h26v20H3z"/><path fill="#fff" d="M9 22v-9l3 3.5L15 13v9h3v-12h-3l-3 3.5L9 11H6v12h3zm12-7h2.5V11h3v4H29l-3.5 5L22 15h-1z"/></svg>`;
  // HTML Icon
  const svgHtml = `<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg"><path fill="#e34f26" d="M4 4l2.1 23.6L16 30l9.9-2.4L28 4H4zm20 6.6H9.1l.3 3.4h14l-.9 10L16 26l-6.5-1.9-.4-4.5h3.4l.2 2 3.3.9 3.3-.9.4-4.5H8.5l-.9-10h16.8l-.4 3.6z"/></svg>`;
  // CSS Icon
  const svgCss = `<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg"><path fill="#1572b6" d="M4 4l2.1 23.6L16 30l9.9-2.4L28 4H4zm18.3 11.2l-.9 10L15.9 27l-6.4-1.9-.3-4h3.4l.2 1.7 3.2.9 3.2-.9.4-4.3H8.3l-.3-3.4h11.2l.2-2.5H7.7l-.3-3.5H23l-.7 8z"/></svg>`;

  // Default File (Text)
  const svgDefault = `<svg viewBox="0 0 32 32" fill="#8b949e" xmlns="http://www.w3.org/2000/svg"><path d="M22 6H10c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2zM10 24V8h12v16H10z"/><path d="M13 12h6v2h-6zm0 4h6v2h-6z"/></svg>`;

  // Default Folder (Closed)
  const svgFolder = `<svg viewBox="0 0 32 32" fill="#dcb67a" xmlns="http://www.w3.org/2000/svg"><path d="M13.3 8H6c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h20c1.1 0 2-.9 2-2V12c0-1.1-.9-2-2-2H15.3l-2-2z"/></svg>`;

  // Open Folder
  const svgFolderOpen = `<svg viewBox="0 0 32 32" fill="#dcb67a" xmlns="http://www.w3.org/2000/svg"><path d="M28.4 12H15.3l-2-2H6c-1.1 0-2 .9-2 2v12c0 1.1.9 2 2 2h21c1.1 0 1.9-.8 2-1.9l1.4-10c.2-1.1-.6-2.1-1.7-2.1z"/></svg>`;

  $: iconSvg = isDir
    ? isOpen
      ? svgFolderOpen
      : svgFolder
    : ext === "svelte"
      ? svgSvelte
      : ext === "ts" || ext === "tsx"
        ? svgTs
        : ext === "js" || ext === "jsx"
          ? svgJs
          : ext === "json"
            ? svgJson
            : ext === "rs"
              ? svgRust
              : ext === "md" || ext === "mdx"
                ? svgMd
                : ext === "html"
                  ? svgHtml
                  : ext === "css" || ext === "scss" || ext === "less"
                    ? svgCss
                    : name === "Cargo.toml"
                      ? svgRust
                      : svgDefault;
</script>

<div class="file-icon">
  {@html iconSvg}
</div>

<style>
  .file-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    flex-shrink: 0;
  }
  .file-icon :global(svg) {
    width: 100%;
    height: 100%;
  }
</style>
