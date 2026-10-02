import { mount } from 'svelte';
import App from './App.svelte';
import './styles.css';
import './theme.css';

mount(App, { target: document.getElementById('app')! });
