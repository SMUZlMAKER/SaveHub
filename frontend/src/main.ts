import { mount } from 'svelte'
import './app.css'
import App from './App.svelte'
import LaunchDialog from './lib/components/LaunchDialog.svelte'

const launchDialog = new URLSearchParams(window.location.search).has('launch')
if (launchDialog) document.body.classList.add('launch-dialog')

const app = mount(launchDialog ? LaunchDialog : App, {
  target: document.getElementById('app')!,
})

export default app
