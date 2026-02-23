# Polkadot REST API Documentation

Modern documentation system for Polkadot REST API with enhanced user experience.

## Architecture

The documentation system is built with vanilla JavaScript and webpack, creating a self-contained deployment that works without a server.

### Core Components

- **main.js** - Application orchestration and navigation
- **parser.js** - OpenAPI specification processing
- **components.js** - UI rendering and DOM manipulation  
- **api-explorer.js** - Interactive "try it out" functionality
- **search.js** - Real-time search across endpoints
- **guides-content.js** - Static guide content management

### Build System

Uses webpack to bundle all assets into a single deployable `dist/` folder containing:
- `index.html` - Complete documentation page
- `bundle.js` - All JavaScript with embedded OpenAPI spec
- `favicon.ico` - Site icon

## Adding Content

### Adding Guides

1. Create a markdown file in `guides/` directory
2. Import it in `scripts/guides-content.js`:
   ```javascript
   import newGuideMd from '../guides/NEW_GUIDE.md';
   ```
3. Add to the exports:
   ```javascript
   export const GUIDES_CONTENT = {
     'new-guide': convertMarkdownToHtml(newGuideMd)
   };
   
   export const GUIDE_METADATA = {
     'new-guide': {
       title: 'New Guide Title',
       description: 'Guide description'
     }
   };
   ```
4. Add navigation link in `index.html`:
   ```html
   <li class="nav-item">
     <a href="#guide-new-guide" class="nav-link" data-guide="new-guide">
       <span>New Guide</span>
     </a>
   </li>
   ```
5. Add content section in `index.html`:
   ```html
   <section id="guide-new-guide" class="content-section" style="display: none;">
     <div class="section-header">
       <h1>New Guide Title</h1>
     </div>
     <div class="guide-content">
       <p class="lead">Guide description</p>
     </div>
   </section>
   ```

### Adding Specifications

Follow the same pattern as guides, but use:
- `data-spec` attribute instead of `data-guide`
- `#spec-` prefix for IDs and URLs
- Add to specifications navigation section

### Markdown Support

The markdown converter supports:
- Headers (h1-h4) with automatic ID generation
- Tables with proper HTML conversion
- Code blocks with syntax highlighting
- Internal links with smooth scrolling
- Bold, italic, and inline code formatting
- Notice boxes and blockquotes

## Running the Documentation

### Development

```bash
cd docs
yarn install
yarn dev    # Development server on localhost:8082
```

### Production Build

```bash
yarn build  # Creates deployable dist/ folder
```

### Deployment

The built `dist/` folder is embedded into the API binary at compile time using `include_dir`. When the API server is running, the documentation is available at:

- **`http://localhost:8080/docs/`** — Interactive documentation UI (trailing slash required)
- **`http://localhost:8080/api-docs/openapi.json`** — Auto-generated OpenAPI 3.0 spec

No separate web server or static hosting is needed — the docs are served directly by the API.

The `dist/` folder can also be:
- Opened directly in a browser (`docs/dist/index.html`)
- Served by any web server
- Deployed to static hosting services

## Configuration

### Server Selection

Default servers are configured in `index.html`:
```html
<select id="server-select">
  <option value="0">Polkadot Public</option>
  <option value="1">Kusama Public</option>
  <option value="2">Polkadot Asset Hub</option>
  <option value="3">Kusama Asset Hub</option>
  <option value="4" selected>Localhost</option>
</select>
```

### Theme

The documentation uses CSS custom properties for theming. Modify `styles/main.css` to change colors and spacing.

## Updating the Documentation

When API endpoints change (new endpoints, updated parameters, etc.), the embedded OpenAPI spec needs to be regenerated:

```bash
# 1. Start the API server locally (it generates the OpenAPI spec at runtime)
SAS_SUBSTRATE_URL=wss://rpc.polkadot.io cargo run --release --bin polkadot-rest-api

# 2. In another terminal, fetch the latest spec from the running server
cd docs
yarn update-spec   # Runs: curl -s http://localhost:8080/api-docs/openapi.json > openapi.json

# 3. Rebuild the docs bundle with the updated spec
yarn build          # Creates updated dist/ folder

# 4. Rebuild the API binary to embed the updated docs
cd ..
cargo build --release --package polkadot-rest-api
```

The `openapi.json` file is generated dynamically by the API from utoipa annotations on handlers. The `openapi-v1.yaml` is a legacy static spec kept for reference.
